"""The two gates together, with fakes in place of both providers."""

import asyncio
import json
import re

import pytest
from livekit.agents import llm

from aule_agent.config import Settings
from aule_agent.events import contracts_dir
from aule_agent.moderation import _PATTERN_DEFINITIONS, THRESHOLDS, Action, Decision, Finding
from aule_agent.physical_safety import Verdict
from aule_agent.replies import ADULT_HELP_SEQUENCE, ANSWERABLE, REPLIES
from aule_agent.safety import (
    _PROVIDER_CATEGORY,
    FLAG_ACTIONS,
    FLAG_CATEGORIES,
    FLAG_GATES,
    Gates,
    audience_of,
    build_gates,
    moderation_categories,
)


class Moderation:
    def __init__(self, finding=None, *, error=None, wait=None):
        self.finding = finding or Finding()
        self.error = error
        self.wait = wait
        self.seen = []
        self.closed = self.cancelled = False

    async def check(self, text):
        self.seen.append(text)
        try:
            if self.wait is not None:
                await self.wait.wait()
        except asyncio.CancelledError:
            self.cancelled = True
            raise
        if self.error is not None:
            raise self.error
        return self.finding

    async def close(self):
        self.closed = True


class Physical:
    def __init__(self, verdict=Verdict.ALLOW, *, error=None, wait=None):
        self.verdict = verdict
        self.error = error
        self.wait = wait
        self.seen = []
        self.closed = self.cancelled = False

    async def check(self, context, message):
        self.seen.append((context, message))
        try:
            if self.wait is not None:
                await self.wait.wait()
        except asyncio.CancelledError:
            self.cancelled = True
            raise
        if self.error is not None:
            raise self.error
        return self.verdict

    async def close(self):
        self.closed = True


def said(text="voy a enchufar esto"):
    return llm.ChatMessage(role="user", content=[text])


def gates(moderation=None, physical=None):
    """Gates with a recorder in place of the platform."""
    flags = []
    built = Gates(
        moderation=moderation,
        physical_safety=physical,
        report=lambda gate, action, categories: flags.append((gate, action, categories)),
    )
    return built, flags


@pytest.mark.parametrize(
    ("metadata", "audience"),
    [
        ('{"audience": "operator"}', "operator"),
        ('{"audience": "child"}', "child"),
        ('{"audience": "OPERATOR"}', "child"),
        ("{}", "child"),
        ("", "child"),
        ("not json", "child"),
        ('["operator"]', "child"),
    ],
)
def test_only_an_explicit_operator_is_not_a_child(metadata, audience):
    assert audience_of(metadata) == audience


def test_a_child_session_needs_both_gates():
    assert not Gates().may_start("child")
    assert not Gates(moderation=Moderation()).may_start("child")
    assert not Gates(physical_safety=Physical()).may_start("child")
    assert Gates(moderation=Moderation(), physical_safety=Physical()).may_start("child")


def test_an_operator_session_may_start_without_gates():
    assert Gates().may_start("operator")


@pytest.mark.parametrize(
    ("keys", "complete", "checks_spoken"),
    [
        ({}, False, False),
        ({"openai_api_key": "sk-test"}, False, True),
        ({"typesafe_api_key": "ts-test"}, False, False),
        ({"openai_api_key": "sk-test", "typesafe_api_key": "ts-test"}, True, True),
    ],
)
async def test_each_key_builds_its_gate_and_nothing_can_turn_one_off(keys, complete, checks_spoken):
    settings = Settings(
        agent_name="a",
        stt="deepgram",
        tts="cartesia",
        tts_voice=None,
        llm_model="m",
        language="es",
        session_max_minutes=15,
        push_to_talk=False,
        **keys,
    )

    built = build_gates(settings)

    assert built.complete is complete is settings.gates_complete
    assert built.checks_spoken is checks_spoken
    assert built.may_start("child") is complete
    await built.close()


async def test_what_is_heard_passes_both_gates_with_its_context():
    moderation, physical = Moderation(), Physical()
    both, flags = gates(moderation, physical)
    context, message = llm.ChatContext.empty(), said()

    assert (await both.heard(context, message)).allowed
    assert moderation.seen == ["voy a enchufar esto"]
    assert physical.seen == [(context, message)]
    assert flags == []


async def test_both_gates_run_at_once_and_the_decision_waits_for_both():
    release = asyncio.Event()
    moderation, physical = Moderation(wait=release), Physical(wait=release)
    both, _ = gates(moderation, physical)

    task = asyncio.create_task(both.heard(llm.ChatContext.empty(), said()))
    while not (moderation.seen and physical.seen):
        await asyncio.sleep(0)
    assert not task.done()

    release.set()
    assert (await task).allowed


async def test_cancelling_a_turn_cancels_both_gates():
    moderation, physical = Moderation(wait=asyncio.Event()), Physical(wait=asyncio.Event())
    both, flags = gates(moderation, physical)
    task = asyncio.create_task(both.heard(llm.ChatContext.empty(), said()))
    while not (moderation.seen and physical.seen):
        await asyncio.sleep(0)

    task.cancel()

    with pytest.raises(asyncio.CancelledError):
        await task
    assert moderation.cancelled and physical.cancelled
    assert flags == []


@pytest.mark.parametrize(
    ("finding", "verdict", "reply", "flag"),
    [
        # Physical safety alone.
        (
            Finding(),
            Verdict.ADULT_HELP,
            "adult_help",
            ("physical_safety", "adult_help", ["physical_hazard"]),
        ),
        (
            Finding(),
            Verdict.UNCERTAIN,
            "clarify",
            ("physical_safety", "clarify", ["physical_hazard"]),
        ),
        (
            Finding(),
            Verdict.UNAVAILABLE,
            "unavailable",
            ("physical_safety", "unavailable", ["gate_unavailable"]),
        ),
        (
            Finding(),
            Verdict.CONTEXT_LIMIT,
            "context_limit",
            ("physical_safety", "unavailable", ["gate_unavailable"]),
        ),
        # Moderation alone.
        (
            Finding(categories=("violence/graphic", "violence")),
            Verdict.ALLOW,
            "redirect",
            ("moderation_input", "redirect", ["violence"]),
        ),
        (
            Finding(categories=("harassment", "self-harm/intent")),
            Verdict.ALLOW,
            "self_harm_support",
            ("moderation_input", "support", ["self_harm", "harassment"]),
        ),
        (
            Finding(categories=("sexual", "sexual/minors")),
            Verdict.ALLOW,
            "child_support",
            ("moderation_input", "support", ["sexual/minors", "sexual"]),
        ),
        (
            Finding(available=False),
            Verdict.ALLOW,
            "unavailable",
            ("moderation_input", "unavailable", ["gate_unavailable"]),
        ),
        # Both: support and a failed moderation keep their reply, and the
        # hazard is still reported in the same flag.
        (
            Finding(categories=("self-harm",)),
            Verdict.ADULT_HELP,
            "self_harm_support",
            ("moderation_input", "support", ["self_harm", "physical_hazard"]),
        ),
        (
            Finding(categories=("sexual/minors",)),
            Verdict.ADULT_HELP,
            "child_support",
            ("moderation_input", "support", ["sexual/minors", "physical_hazard"]),
        ),
        (
            Finding(available=False),
            Verdict.ADULT_HELP,
            "unavailable",
            ("moderation_input", "unavailable", ["physical_hazard", "gate_unavailable"]),
        ),
        (
            Finding(categories=("violence",)),
            Verdict.ADULT_HELP,
            "adult_help",
            ("physical_safety", "adult_help", ["physical_hazard", "violence"]),
        ),
        (
            Finding(categories=("violence",)),
            Verdict.UNCERTAIN,
            "redirect",
            ("moderation_input", "redirect", ["violence"]),
        ),
        (
            Finding(categories=("violence",)),
            Verdict.UNAVAILABLE,
            "unavailable",
            ("physical_safety", "unavailable", ["gate_unavailable", "violence"]),
        ),
        # Swearing alone does not stop a child, and is not what gets reported
        # when something else does.
        (
            Finding(profanity=True),
            Verdict.UNCERTAIN,
            "clarify",
            ("physical_safety", "clarify", ["physical_hazard"]),
        ),
    ],
)
async def test_a_stopped_turn_gets_its_reply_and_one_flag(finding, verdict, reply, flag):
    both, flags = gates(Moderation(finding), Physical(verdict))

    decision = await both.heard(llm.ChatContext.empty(), said())

    assert not decision.allowed
    assert decision.reply == reply
    assert flags == [flag]


async def test_a_child_who_swears_is_answered():
    both, flags = gates(Moderation(Finding(profanity=True)), Physical())

    assert (await both.heard(llm.ChatContext.empty(), said())).allowed
    assert flags == []


@pytest.mark.parametrize(
    ("moderation", "physical", "gate"),
    [
        (Moderation(error=RuntimeError("bug")), Physical(wait=asyncio.Event()), "moderation_input"),
        (Moderation(wait=asyncio.Event()), Physical(error=RuntimeError("bug")), "physical_safety"),
    ],
)
async def test_a_gate_that_breaks_says_no_and_cancels_the_other(moderation, physical, gate):
    both, flags = gates(moderation, physical)

    decision = await both.heard(llm.ChatContext.empty(), said())

    assert decision == Decision(Action.UNAVAILABLE, "unavailable")
    assert flags == [(gate, "unavailable", ["gate_unavailable"])]
    assert moderation.cancelled or physical.cancelled


async def test_what_is_spoken_passes_moderation_only():
    moderation, physical = Moderation(), Physical(Verdict.ADULT_HELP)
    both, flags = gates(moderation, physical)

    assert (await both.spoken("muy bien")).allowed
    assert moderation.seen == ["muy bien"]
    assert physical.seen == []
    assert flags == []


@pytest.mark.parametrize(
    ("moderation", "reply", "flag"),
    [
        (
            Moderation(Finding(categories=("sexual",), profanity=True)),
            "redirect",
            ("moderation_output", "redirect", ["sexual", "profanity"]),
        ),
        (
            Moderation(Finding(profanity=True)),
            "redirect",
            ("moderation_output", "redirect", ["profanity"]),
        ),
        (
            Moderation(Finding(available=False)),
            "unavailable",
            ("moderation_output", "unavailable", ["gate_unavailable"]),
        ),
        (
            Moderation(error=RuntimeError("bug")),
            "unavailable",
            ("moderation_output", "unavailable", ["gate_unavailable"]),
        ),
    ],
)
async def test_a_reply_that_does_not_pass_is_replaced_and_flagged(moderation, reply, flag):
    both, flags = gates(moderation, Physical())

    decision = await both.spoken("algo que el tutor no debe decir")

    assert decision.reply == reply
    assert flags == [flag]


async def test_in_an_operator_session_a_missing_gate_is_skipped():
    physical_only, flags = gates(physical=Physical(Verdict.ADULT_HELP))
    assert (await physical_only.heard(llm.ChatContext.empty(), said())).reply == "adult_help"
    assert (await physical_only.spoken("x")).allowed
    assert not physical_only.checks_spoken
    assert flags == [("physical_safety", "adult_help", ["physical_hazard"])]

    moderation_only, flags = gates(Moderation(Finding(categories=("hate",))))
    assert (await moderation_only.heard(llm.ChatContext.empty(), said())).reply == "redirect"
    assert flags == [("moderation_input", "redirect", ["hate"])]

    assert (await Gates().heard(llm.ChatContext.empty(), said())).allowed
    assert (await Gates().spoken("x")).allowed


async def test_silence_is_not_sent_to_any_provider():
    moderation, physical = Moderation(), Physical()
    both, _ = gates(moderation, physical)

    assert (await both.heard(llm.ChatContext.empty(), said("  "))).allowed
    assert (await both.spoken(" ")).allowed
    assert moderation.seen == [] and physical.seen == []


async def test_a_stopped_turn_is_logged_by_category_only(caplog):
    both, _ = gates(Moderation(Finding(categories=("violence",))), Physical())

    with caplog.at_level("DEBUG"):
        await both.heard(llm.ChatContext.empty(), said("secret words of a child"))

    (record,) = caplog.records
    assert (record.gate, record.action, record.categories) == (
        "moderation_input",
        "redirect",
        ["violence"],
    )
    assert "secret" not in str(record.__dict__)


async def test_closing_the_gates_closes_both():
    moderation, physical = Moderation(), Physical()

    await Gates(moderation=moderation, physical_safety=physical).close()

    assert moderation.closed and physical.closed


def test_every_category_a_gate_can_find_has_a_flag_category():
    found = {*THRESHOLDS, *(category for category, _ in _PATTERN_DEFINITIONS)} - {"profanity"}

    assert found == set(_PROVIDER_CATEGORY)
    assert set(_PROVIDER_CATEGORY.values()) < set(FLAG_CATEGORIES)
    assert moderation_categories(
        Finding(available=False, categories=tuple(found), profanity=True)
    ) == [category for category in FLAG_CATEGORIES if category != "physical_hazard"]


def test_the_flag_vocabulary_fits_the_safety_flagged_contract():
    path = contracts_dir() / "events" / "safety.flagged.v1.schema.json"
    schema = json.loads(path.read_text(encoding="utf-8"))["properties"]

    assert FLAG_GATES <= set(schema["gate"]["enum"])
    assert FLAG_ACTIONS <= set(schema["action"]["enum"])
    assert len(set(FLAG_CATEGORIES)) == len(FLAG_CATEGORIES)
    pattern = schema["categories"]["items"]["pattern"]
    assert all(re.fullmatch(pattern, category) for category in FLAG_CATEGORIES)


def test_every_reply_exists_in_every_language():
    assert all(set(texts) == {"es", "en"} for texts in REPLIES.values())
    assert all(text.strip() for texts in REPLIES.values() for text in texts.values())
    assert set(ADULT_HELP_SEQUENCE) | ANSWERABLE <= set(REPLIES)


@pytest.mark.parametrize("finding", [Finding(available=False), Finding(categories=("hate",))])
@pytest.mark.parametrize("verdict", list(Verdict))
async def test_every_reply_a_decision_can_name_exists(finding, verdict):
    both, _ = gates(Moderation(finding), Physical(verdict))

    assert (await both.heard(llm.ChatContext.empty(), said())).reply in REPLIES
    assert (await both.spoken("x")).reply in REPLIES


class Opinion:
    """Stands in for the second reader of an unsure reply."""

    def __init__(self, safe):
        self.answer, self.read, self.closed = safe, [], False

    async def safe(self, text):
        self.read.append(text)
        return self.answer

    async def ordinary(self, tutor, child):
        self.read.append((tutor, child))
        return self.answer

    async def close(self):
        self.closed = True


def unsure_gates(opinion, finding=None):
    flags = []
    finding = finding or Finding(categories=("sexual",), unsure=True)
    built = Gates(
        moderation=Moderation(finding),
        physical_safety=Physical(),
        report=lambda gate, action, categories: flags.append((gate, action, categories)),
        second_opinion=opinion,
    )
    return built, flags


async def test_an_unsure_reply_is_spoken_only_when_the_second_reader_says_it_is_safe():
    safe, unsafe = Opinion(True), Opinion(False)
    allowed, no_flags = unsure_gates(safe)
    stopped, flags = unsure_gates(unsafe)

    assert (await allowed.spoken("cuenta las canicas con el dedo")).allowed
    assert no_flags == [] and safe.read == ["cuenta las canicas con el dedo"]
    assert (await stopped.spoken("otra cosa")).reply == "redirect"
    assert [flag[:2] for flag in flags] == [("moderation_output", "redirect")]


async def test_without_a_second_reader_an_unsure_reply_stays_stopped():
    stopped, flags = unsure_gates(None)

    assert not (await stopped.spoken("cuenta las canicas")).allowed
    assert len(flags) == 1


async def test_what_the_provider_is_sure_about_never_gets_a_second_reading():
    opinion = Opinion(True)
    for finding in (
        Finding(categories=("sexual",)),
        Finding(categories=("sexual", "violence"), unsure=False),
        Finding(available=False),
    ):
        stopped, _ = unsure_gates(opinion, finding)
        assert not (await stopped.spoken("x")).allowed
    assert opinion.read == []


async def test_ordinary_words_the_provider_was_unsure_about_are_answered():
    # "I'm back, I stretched" scores as sexual; the provider did not act on it.
    reader = Opinion(True)
    built, flags = unsure_gates(reader)

    decision = await built.heard(after("Ponte de pie y estírate."), said("ya volví, ya me estiré"))

    assert decision.allowed and flags == []
    assert reader.read == [("Ponte de pie y estírate.", "ya volví, ya me estiré")]


@pytest.mark.parametrize("answer", [False, None])
async def test_unsure_words_the_reader_does_not_clear_stay_stopped(answer):
    built, flags = unsure_gates(Opinion(answer))

    decision = await built.heard(after("¿Seguimos?"), said("x"))

    assert decision.reply == "redirect"
    assert [flag[:2] for flag in flags] == [("moderation_input", "redirect")]


async def test_the_second_reader_is_closed_with_the_gates():
    opinion = Opinion(True)
    built, _ = unsure_gates(opinion)

    await built.close()

    assert opinion.closed


FAINT = Finding(categories=("self-harm", "self-harm/intent"), faint_self_harm=True)
# A faint score under our thresholds: nothing was found.
UNDER = Finding(faint_self_harm=True)


def faint_gates(opinion, verdict=Verdict.ALLOW, finding=FAINT):
    flags = []
    built = Gates(
        moderation=Moderation(finding),
        physical_safety=Physical(verdict),
        report=lambda gate, action, categories: flags.append((gate, action, categories)),
        second_opinion=opinion,
    )
    return built, flags


def after(tutor_line):
    context = llm.ChatContext.empty()
    context.add_message(role="assistant", content="¡Hola!")
    context.add_message(role="user", content="hola")
    context.add_message(role="assistant", content=tutor_line)
    return context


async def test_a_tired_student_is_answered_when_the_second_reader_says_it_is_about_the_work():
    reader = Opinion(True)
    built, flags = faint_gates(reader)

    decision = await built.heard(after("¿Hacemos otro ejercicio?"), said("ya no quiero seguir"))

    assert decision.allowed and flags == []
    # The reader gets the sentence with the tutor's line right before it, and nothing else.
    assert reader.read == [("¿Hacemos otro ejercicio?", "ya no quiero seguir")]


@pytest.mark.parametrize("answer", [False, None])
async def test_a_faint_score_over_our_thresholds_the_reader_does_not_clear_keeps_the_line(answer):
    built, flags = faint_gates(Opinion(answer))

    decision = await built.heard(after("¿Seguimos?"), said("no quiero despertar mañana"))

    assert (decision.action, decision.reply) == (Action.SUPPORT, "self_harm_support")
    assert [flag[:2] for flag in flags] == [("moderation_input", "support")]


async def test_a_faint_score_the_other_gate_also_stops_keeps_the_line_for_distress():
    # "I don't want to wake up tomorrow" scores faintly and worries the
    # physical-safety gate too: the reader sees a concern, and the family is
    # told of self-harm, not of a hazard.
    reader = Opinion(False)
    built, flags = faint_gates(reader, Verdict.ADULT_HELP)

    decision = await built.heard(after("¿Seguimos?"), said("no quiero despertar mañana"))

    assert (decision.action, decision.reply) == (Action.SUPPORT, "self_harm_support")
    assert flags == [("moderation_input", "support", ["self_harm", "physical_hazard"])]


async def test_veiled_words_the_other_gate_is_unsure_about_get_the_line_for_distress():
    # "Ojalá no hubiera nacido": nothing over our thresholds, and the
    # physical-safety gate unsure; without the reading it would hear "say
    # it another way".
    built, _ = faint_gates(Opinion(False), Verdict.UNCERTAIN, UNDER)

    decision = await built.heard(
        after("No pasa nada si fallamos."), said("ojalá no hubiera nacido")
    )

    assert (decision.action, decision.reply) == (Action.SUPPORT, "self_harm_support")


@pytest.mark.parametrize("verdict", [Verdict.UNAVAILABLE, Verdict.CONTEXT_LIMIT])
async def test_a_turn_the_other_gate_could_not_judge_is_not_read_again(verdict):
    reader = Opinion(True)
    built, _ = faint_gates(reader, verdict)

    decision = await built.heard(after("¿Seguimos?"), said("ya no quiero seguir"))

    assert decision.reply == "self_harm_support" and reader.read == []


async def test_self_harm_that_is_not_faint_is_never_read_again():
    reader = Opinion(True)
    for finding in (
        Finding(categories=("self-harm",)),
        Finding(categories=("self-harm/intent", "violence")),
    ):
        for verdict in (Verdict.ALLOW, Verdict.ADULT_HELP):
            built, _ = faint_gates(reader, verdict, finding)
            decision = await built.heard(after("¿Seguimos?"), said("x"))
            assert decision.reply == "self_harm_support"
    assert reader.read == []


async def test_without_a_second_reader_a_faint_score_keeps_the_line_for_distress():
    built, _ = faint_gates(None)

    decision = await built.heard(after("¿Hacemos otro?"), said("ya no quiero seguir"))

    assert decision.reply == "self_harm_support"


async def test_veiled_words_under_our_thresholds_are_stopped_when_two_readers_see_a_concern():
    # "Todos estarían mejor sin mí" scores 0.04: nothing over our thresholds.
    reader = Opinion(False)
    built, flags = faint_gates(reader, finding=UNDER)

    decision = await built.heard(after("Muy bien, sigamos."), said("todos estarían mejor sin mí"))

    assert (decision.action, decision.reply) == (Action.SUPPORT, "self_harm_support")
    assert len(reader.read) == 2
    assert [flag[:2] for flag in flags] == [("moderation_input", "support")]


class Split:
    """Two readings that do not agree."""

    def __init__(self, *answers):
        self.answers, self.read = list(answers), []

    async def ordinary(self, tutor, child):
        self.read.append(child)
        return self.answers.pop(0)

    async def close(self):
        pass


@pytest.mark.parametrize("answers", [(False, None), (False, True), (None, None), (True, True)])
async def test_under_our_thresholds_anything_short_of_two_concerns_lets_the_turn_through(answers):
    built, flags = faint_gates(Split(*answers), finding=UNDER)

    decision = await built.heard(after("¿Seguimos?"), said("ñañaña, no me importa"))

    assert decision.allowed and flags == []
