"""The safety gates of a session, and who a session is for.

ARCHITECTURE.md, "Safety and privacy requirements": every turn passes content
moderation (``moderation``) and a physical-safety check (``physical_safety``),
both fail closed, and a child's session does not start unless both are
configured. In an operator's session (an adult on the team, marked as such by
the server that issued the room token) a gate that is not configured is skipped.

When a gate stops a turn the person hears a fixed reply (``replies``) and the
platform is told which gate, what it did and the categories involved, in the
vocabulary of the ``safety.flagged`` contract. Nothing else leaves this module:
no words, no scores.
"""

import asyncio
import json
import logging
from collections.abc import Callable
from dataclasses import replace
from typing import Literal

from livekit.agents import llm

from .config import Settings
from .moderation import Action, Decision, Finding, Moderator, classify_heard, classify_spoken
from .opinion import SecondOpinion
from .physical_safety import PhysicalSafetyGuard, Verdict, combine_input_decisions

logger = logging.getLogger("aule.agent.safety")

Audience = Literal["operator", "child"]

# The vocabulary of a flag. Gates and actions are those of
# contracts/events/safety.flagged.v1.schema.json that this agent produces; the
# categories are ours, and every provider category is mapped onto one of them.
FLAG_GATES = frozenset({"moderation_input", "moderation_output", "physical_safety"})
FLAG_ACTIONS = frozenset({"redirect", "adult_help", "clarify", "support", "unavailable"})
FLAG_CATEGORIES = (
    # Ordered by gravity: the first category of a flag is the one that matters most.
    "self_harm",
    "sexual/minors",
    "physical_hazard",
    "gate_unavailable",
    "sexual",
    "violence",
    "harassment",
    "hate",
    "illicit",
    "prompt_injection",
    "profanity",
)
_PROVIDER_CATEGORY = {
    "self-harm": "self_harm",
    "self-harm/intent": "self_harm",
    "self-harm/instructions": "self_harm",
    "sexual": "sexual",
    "sexual/minors": "sexual/minors",
    "violence": "violence",
    "violence/graphic": "violence",
    "harassment": "harassment",
    "harassment/threatening": "harassment",
    "hate": "hate",
    "hate/threatening": "hate",
    "illicit": "illicit",
    "illicit/violent": "illicit",
    "prompt_injection": "prompt_injection",
}

# Receives (gate, action, categories). Must not wait and must not raise.
Report = Callable[[str, str, list[str]], None]


def audience_of(dispatch_metadata: str) -> Audience:
    """Who the session is for. Anything not explicitly an operator is a child."""
    try:
        metadata = json.loads(dispatch_metadata or "{}")
    except ValueError:
        return "child"
    if isinstance(metadata, dict) and metadata.get("audience") == "operator":
        return "operator"
    return "child"


def flag_categories(*found: str) -> list[str]:
    """The given flag categories without repeats, gravest first."""
    return [category for category in FLAG_CATEGORIES if category in found]


def moderation_categories(finding: Finding) -> list[str]:
    """What moderation found, in the vocabulary of a flag."""
    found = [_PROVIDER_CATEGORY[category] for category in finding.categories]
    if finding.profanity:
        found.append("profanity")
    if not finding.available:
        found.append("gate_unavailable")
    return flag_categories(*found)


# How much of the tutor's last line the second reader is given: enough to tell
# what the child was answering.
MAX_TUTOR_LINE_CHARS = 400


def _last_said_by_the_tutor(context: llm.ChatContext) -> str:
    for item in reversed(context.items):
        if item.type == "message" and item.role == "assistant":
            return (item.text_content or "")[-MAX_TUTOR_LINE_CHARS:]
    return ""


class Gates:
    """The two gates of a session. Either may be missing in an operator's session."""

    def __init__(
        self,
        *,
        moderation: Moderator | None = None,
        physical_safety: PhysicalSafetyGuard | None = None,
        report: Report | None = None,
        second_opinion: SecondOpinion | None = None,
    ) -> None:
        self._second_opinion = second_opinion
        self._moderation = moderation
        self._physical_safety = physical_safety
        self._report = report

    @property
    def complete(self) -> bool:
        return self._moderation is not None and self._physical_safety is not None

    def may_start(self, audience: Audience) -> bool:
        return audience == "operator" or self.complete

    @property
    def checks_spoken(self) -> bool:
        return self._moderation is not None

    async def heard(self, context: llm.ChatContext, message: llm.ChatMessage) -> Decision:
        """Judges what the person said, before the tutor answers it.

        Both gates run at the same time and the decision waits for both.
        """
        text = message.raw_text_content or ""
        if not text.strip() or (self._moderation is None and self._physical_safety is None):
            return Decision(Action.ALLOW)
        moderating: asyncio.Task[Finding] | None = None
        judging: asyncio.Task[Verdict] | None = None
        try:
            # The group cancels one gate if the other fails unexpectedly.
            async with asyncio.TaskGroup() as group:
                if self._moderation is not None:
                    moderating = group.create_task(self._moderation.check(text))
                if self._physical_safety is not None:
                    judging = group.create_task(self._physical_safety.check(context, message))
        except ExceptionGroup as errors:
            failed = judging is not None and not judging.cancelled() and judging.exception()
            return self._broken("physical_safety" if failed else "moderation_input", errors)
        finding = moderating.result() if moderating else None
        verdict = judging.result() if judging else None

        if finding is not None:
            finding = await self._read_again(finding, context, text, verdict)
        existing = classify_heard(finding) if finding else Decision(Action.ALLOW)
        decision = combine_input_decisions(existing, verdict) if verdict else existing
        if decision.allowed:
            return decision

        found = moderation_categories(finding) if finding and not existing.allowed else []
        if verdict is Verdict.ADULT_HELP:
            found.append("physical_hazard")
        if decision == existing:
            self._flag("moderation_input", decision.action.value, found)
        elif verdict is Verdict.ADULT_HELP:
            self._flag("physical_safety", "adult_help", found)
        elif verdict is Verdict.UNCERTAIN:
            self._flag("physical_safety", "clarify", ["physical_hazard"])
        else:
            self._flag("physical_safety", "unavailable", [*found, "gate_unavailable"])
        return decision

    async def _read_again(
        self, finding: Finding, context: llm.ChatContext, text: str, verdict: Verdict | None
    ) -> Finding:
        """What moderation found, once a second reader has settled what a
        score could not.

        A sentence is read again when the provider did not act on it and all
        it found is either a faint self-harm score or one of the categories
        where ordinary words score high (see ``Finding``). Over our thresholds,
        only a clear "ordinary" lets it through. Under them, two readers are
        asked at once, and a "concern" that the other reader does not contradict
        stops it with the line for a child in distress. Otherwise the
        finding stays as it was. When the physical-safety gate could not judge
        the turn at all, it is stopped anyway and is not read again.
        """
        if not (finding.faint_self_harm or finding.unsure) or self._second_opinion is None:
            return finding
        if verdict in (Verdict.UNAVAILABLE, Verdict.CONTEXT_LIMIT):
            return finding
        tutor = _last_said_by_the_tutor(context)
        if finding.categories:
            # Stopped unless the reader is clear that it is ordinary.
            ordinary = await self._second_opinion.ordinary(tutor, text)
        else:
            # Nothing over our thresholds: two readers at once, and it is a
            # concern when one says so and neither says it is ordinary. One
            # reader alone hears worry in "ñañaña, no me importa" two times in
            # five; a reader that is late must not let a veiled sentence pass.
            readings = await asyncio.gather(
                self._second_opinion.ordinary(tutor, text),
                self._second_opinion.ordinary(tutor, text),
            )
            concern = False in readings and True not in readings
            ordinary = False if concern else None
        # Counts only: what was said is never logged.
        logger.info("read again", extra={"ordinary": ordinary, "found": list(finding.categories)})
        if ordinary is True:
            return Finding()
        if ordinary is False and not finding.categories:
            return replace(finding, categories=("self-harm",))
        return finding

    async def spoken(self, text: str) -> Decision:
        """Judges what the tutor is about to say, before any of it is spoken."""
        if self._moderation is None or not text.strip():
            return Decision(Action.ALLOW)
        try:
            finding = await self._moderation.check(text)
        except Exception as error:
            return self._broken("moderation_output", error)
        decision = classify_spoken(finding)
        if not decision.allowed and finding.unsure and self._second_opinion is not None:
            # The provider did not act on it and a score cannot tell a teaching
            # sentence from a bad one: one more reader decides, and only a clear
            # yes lets the reply through.
            if await self._second_opinion.safe(text):
                return Decision(Action.ALLOW)
        if not decision.allowed:
            self._flag("moderation_output", decision.action.value, moderation_categories(finding))
        return decision

    def _broken(self, gate: str, error: BaseException) -> Decision:
        """A gate that fails in a way it did not foresee still says no."""
        logger.error("safety gate failed", extra={"gate": gate, "error": type(error).__name__})
        self._flag(gate, "unavailable", ["gate_unavailable"])
        return Decision(Action.UNAVAILABLE, "unavailable")

    def _flag(self, gate: str, action: str, found: list[str]) -> None:
        categories = flag_categories(*found)
        logger.info(
            "turn stopped by a safety gate",
            extra={"gate": gate, "action": action, "categories": categories},
        )
        if self._report is not None:
            self._report(gate, action, categories)

    async def close(self) -> None:
        if self._moderation is not None:
            await self._moderation.close()
        if self._physical_safety is not None:
            await self._physical_safety.close()
        if self._second_opinion is not None:
            await self._second_opinion.close()


def build_gates(settings: Settings, report: Report | None = None) -> Gates:
    """The gates this worker is configured for. A gate with a key always runs."""
    return Gates(
        moderation=Moderator(settings.openai_api_key) if settings.openai_api_key else None,
        physical_safety=(
            PhysicalSafetyGuard(settings.typesafe_api_key) if settings.typesafe_api_key else None
        ),
        report=report,
        # Only what the tutor is about to say gets a second reader, and only
        # where there is a moderation gate to be unsure.
        second_opinion=SecondOpinion() if settings.openai_api_key else None,
    )
