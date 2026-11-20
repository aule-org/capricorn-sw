"""Content moderation: what a person said, and what the tutor is about to say.

Two layers. A local list of patterns catches a few phrases that must never
depend on a provider being reachable (self-harm, threats, attempts to make the
tutor drop its rules, swearing). OpenAI's moderation endpoint judges everything
else, against thresholds set for children that are far stricter than the
provider's own.

A check that could not be completed is reported as unavailable and is never
read as safe. Only categories and durations are logged: no text, no scores.
"""

import asyncio
import logging
import math
import re
import time
import unicodedata
from dataclasses import dataclass
from enum import StrEnum
from typing import Any

logger = logging.getLogger("aule.agent.moderation")

MODEL = "omni-moderation-latest"
MAX_CHARS = 16_000
TIMEOUT_SECONDS = 2.0
# The first check of a session also opens the connection. It gets one longer
# attempt rather than a first turn that always fails.
INITIAL_TIMEOUT_SECONDS = 8.0

_PATTERN_DEFINITIONS: list[tuple[str, str]] = [
    # Self-harm and aggression come first so the gravest match wins.
    ("self-harm/instructions", r"\bm[aá]tate\b"),
    ("self-harm", r"\bsu[ií]c[ií]d\w*\b"),
    ("self-harm/intent", r"\bme\s+quiero\s+morir\b"),
    ("self-harm/intent", r"\b(?:i\s+want\s+to\s+die|quero\s+morrer)\b"),
    ("harassment", r"\bte\s+odio\b"),
    ("violence", r"\bte\s+voy\s+a\s+matar\b"),
    (
        "prompt_injection",
        r"\b(?:ignora|olvida|omite)\s+(?:todas?\s+)?(?:tus|las)\s+(?:instrucciones|reglas)\b",
    ),
    (
        "prompt_injection",
        r"\b(?:ignore|forget|disregard)\s+(?:all\s+)?(?:your|the)\s+(?:instructions|rules)\b",
    ),
    (
        "prompt_injection",
        r"\b(?:muestra|revela|imprime)\s+(?:tu|el)\s+"
        r"(?:system\s+prompt|prompt\s+del\s+sistema|instrucciones\s+internas)\b",
    ),
    (
        "prompt_injection",
        r"\b(?:show|reveal|print)\s+(?:your|the)\s+(?:system\s+prompt|hidden\s+instructions)\b",
    ),
    ("prompt_injection", r"\bact[uú]a\s+como\s+si\s+no\s+tuvieras\s+reglas\b"),
    *[
        ("profanity", pattern)
        for pattern in [
            r"\bmi[e3]rda\b",
            r"\bput[ao]s?\b",
            r"\bpend[e3]j[ao]s?\b",
            r"\bch[iu]ng[ao]+\b",
            r"\bcul[oe]s?\b",
            r"\bver[gq]a\b",
            r"\bc[ao]j[ou]d[ao]\b",
            r"\bhij[ou][e3]?\s*(?:de\s+)?put[ao]\b",
            r"\bmal\s*parid[ao]\b",
            r"\bcar[a]j[o]\b",
            r"\bjod[ae3ir]\w*\b",
            r"\bconch[ae]\s*(?:de\s+)?(?:tu|su)\b",
            r"\bmaric[oó]n\b",
            r"\bhue?v[oó]n\b",
            r"\bimb[eé]cil\b",
            r"\best[uú]pid[ao]s?\b",
            r"\bidiot[ao]s?\b",
            r"\bweb[oó]n\b",
            r"\bchucha\b",
            r"\bconchatumadre\b",
            r"\bctm\b",
            r"\bptm\b",
            r"\bcsm\b",
            r"\bmaldito\b",
            r"\bpit[ao]\b",
            r"\bping[ao]\b",
            r"\bmam[oó]n\b",
            r"\bfuck\w*\b",
            r"\bsh[i1]t\b",
            r"\bb[i1]tch\b",
            r"\bass\s*hole\b",
            r"\bdamn\b",
            r"\bwh[o0]re\b",
            r"\bd[i1]ck\b",
        ]
    ],
]

_PATTERNS = [
    (category, re.compile(pattern, re.IGNORECASE)) for category, pattern in _PATTERN_DEFINITIONS
]

# Our policy for children, not calibrated probabilities or the provider's defaults.
THRESHOLDS: dict[str, float] = {
    "harassment": 0.3,
    "harassment/threatening": 0.2,
    "self-harm": 0.15,
    "self-harm/intent": 0.1,
    "self-harm/instructions": 0.1,
    "sexual": 0.15,
    "sexual/minors": 0.05,
    "violence": 0.3,
    "violence/graphic": 0.2,
    "hate": 0.3,
    "hate/threatening": 0.2,
    "illicit": 0.2,
    "illicit/violent": 0.1,
}


class Action(StrEnum):
    ALLOW = "allow"
    REDIRECT = "redirect"
    SUPPORT = "support"
    UNAVAILABLE = "unavailable"


@dataclass(frozen=True)
class Decision:
    """What happens to a turn. ``reply`` is a key of ``replies.REPLIES``."""

    action: Action
    reply: str | None = None

    @property
    def allowed(self) -> bool:
        return self.action is Action.ALLOW


@dataclass(frozen=True)
class Finding:
    """What moderation found in one text: provider category names, never the text."""

    available: bool = True
    categories: tuple[str, ...] = ()
    # Swearing is kept apart: a child who swears is not stopped, the tutor is.
    profanity: bool = False
    # True when all that was found is a sexual score over our threshold that the
    # provider itself did not act on. Ordinary teaching sentences land here as
    # often as bad ones, so a score cannot settle it (see ``opinion``).
    unsure: bool = False

    @property
    def flagged(self) -> bool:
        return bool(self.categories) or self.profanity


def _normalize(text: str) -> str:
    """Lower case without accents or invisible characters, so they cannot hide a word."""
    decomposed = unicodedata.normalize("NFKD", text.lower())
    return "".join(
        char
        for char in decomposed
        if not unicodedata.combining(char) and unicodedata.category(char) != "Cf"
    )


def local_category(text: str) -> str | None:
    """The category of the first local pattern found in the text, if any."""
    normalized = _normalize(text)
    for category, pattern in _PATTERNS:
        if pattern.search(normalized):
            return category
    return None


def provider_is_unsure(response: Any) -> bool:
    """Whether the only thing found is "sexual", by our threshold and not by the
    provider's own verdict. Call after ``provider_categories`` accepted the response."""
    result = response.results[0]
    return (
        provider_categories(response) == ("sexual",)
        and result.flagged is False
        and result.categories.sexual is False
    )


def provider_categories(response: Any) -> tuple[str, ...]:
    """The categories over our thresholds. Raises on anything not fully understood."""
    result = response.results[0]
    if not isinstance(result.flagged, bool):
        raise ValueError("invalid moderation verdict")
    found = []
    for category, threshold in THRESHOLDS.items():
        name = category.replace("/", "_").replace("-", "_")
        score = getattr(result.category_scores, name, None)
        if isinstance(score, bool) or not isinstance(score, (int, float)):
            raise ValueError("invalid moderation score")
        if not math.isfinite(score) or not 0 <= score <= 1:
            raise ValueError("invalid moderation score")
        named = getattr(result.categories, name, None)
        if not isinstance(named, bool):
            raise ValueError("invalid moderation category")
        if score >= threshold or named:
            found.append(category)
    if result.flagged and not found:
        # Flagged for a reason we do not know: an answer we cannot classify.
        raise ValueError("moderation flagged without a known category")
    return tuple(found)


class Moderator:
    """Moderation for one session: one client, reused for every check."""

    def __init__(self, api_key: str, *, client: Any = None) -> None:
        if client is None:
            from openai import AsyncOpenAI

            client = AsyncOpenAI(api_key=api_key, timeout=INITIAL_TIMEOUT_SECONDS, max_retries=0)
        self._client = client
        self._connected = False

    async def check(self, text: str) -> Finding:
        started = time.monotonic()
        local = local_category(text)
        categories: tuple[str, ...] = ()
        available = False
        unsure = False
        # A local match never skips the provider: swearing must not hide a
        # graver category in the same sentence.
        if len(text) <= MAX_CHARS:
            timeout = TIMEOUT_SECONDS if self._connected else INITIAL_TIMEOUT_SECONDS
            self._connected = True
            try:
                async with asyncio.timeout(timeout):
                    response = await self._client.moderations.create(model=MODEL, input=text)
                categories = provider_categories(response)
                unsure = provider_is_unsure(response)
                available = True
            except asyncio.CancelledError:
                raise
            except Exception as error:  # whatever went wrong, the text was not checked
                logger.warning("moderation unavailable", extra={"error": type(error).__name__})
        if local and local != "profanity" and local not in categories:
            categories = (*categories, local)
        finding = Finding(
            available=available,
            categories=categories,
            profanity=local == "profanity",
            # A word of our own list is a finding of its own: nothing to be unsure about.
            unsure=unsure and local is None,
        )
        if finding.flagged or not finding.available:
            logger.info(
                "moderation finding",
                extra={
                    "categories": list(finding.categories),
                    "profanity": finding.profanity,
                    "available": finding.available,
                    "duration_ms": round((time.monotonic() - started) * 1000),
                },
            )
        return finding

    async def close(self) -> None:
        await self._client.close()


def classify_heard(finding: Finding) -> Decision:
    """What to do with something a person said. Supportive, never punitive."""
    if any(category.startswith("self-harm") for category in finding.categories):
        return Decision(Action.SUPPORT, "self_harm_support")
    if not finding.available:
        return Decision(Action.UNAVAILABLE, "unavailable")
    if "sexual/minors" in finding.categories:
        return Decision(Action.SUPPORT, "child_support")
    if finding.categories:
        return Decision(Action.REDIRECT, "redirect")
    return Decision(Action.ALLOW)


def classify_spoken(finding: Finding) -> Decision:
    """What to do with a reply of the tutor. Flagged or unchecked text is never spoken."""
    if not finding.available:
        return Decision(Action.UNAVAILABLE, "unavailable")
    if finding.flagged:
        return Decision(Action.REDIRECT, "redirect")
    return Decision(Action.ALLOW)
