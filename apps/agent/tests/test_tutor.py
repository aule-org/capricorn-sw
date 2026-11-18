from types import SimpleNamespace
from unittest.mock import PropertyMock, patch

import pytest
from livekit import rtc
from livekit.agents import StopResponse, llm

from capricorn_agent.moderation import Action, Decision
from capricorn_agent.replies import REPLIES
from capricorn_agent.tutor import (
    INSTRUCTIONS,
    Persona,
    Tutor,
    checked,
    persona_from,
    persona_notes,
    with_frame,
)
from capricorn_agent.worker import GOODBYE, language_of, persona_of


def frame():
    return rtc.VideoFrame(2, 2, rtc.VideoBufferType.RGBA, bytes(16))


def context():
    chat_ctx = llm.ChatContext()
    chat_ctx.add_message(role="user", content="mira mi cuaderno")
    chat_ctx.add_message(role="assistant", content="¿qué intentaste?")
    chat_ctx.add_message(role="user", content="esto")
    return chat_ctx


def images(message):
    return [part for part in message.content if isinstance(part, llm.ImageContent)]


def test_the_frame_goes_on_the_last_user_message_only():
    seen = with_frame(context(), frame())

    first, _, last = seen.items
    assert images(first) == []
    assert len(images(last)) == 1
    assert last.text_content == "esto"


def test_the_session_history_is_left_without_images():
    chat_ctx = context()

    with_frame(chat_ctx, frame())

    assert all(images(item) == [] for item in chat_ctx.items)


def test_without_a_frame_or_a_user_message_the_context_is_unchanged():
    chat_ctx = context()
    assert with_frame(chat_ctx, None) is chat_ctx

    empty = llm.ChatContext()
    assert with_frame(empty, frame()) is empty


def test_every_language_has_its_texts():
    assert set(INSTRUCTIONS) == set(GOODBYE) == {"es", "en"}


def test_language_comes_from_the_dispatch_when_supported():
    assert language_of('{"language": "en"}', "es") == "en"
    assert language_of('{"language": "fr"}', "es") == "es"
    assert language_of("not json", "es") == "es"
    assert language_of("", "en") == "en"


class ScriptedGates:
    """Stands in for the gates: answers each turn with the next decision."""

    checks_spoken = True

    def __init__(self, *decisions):
        self.decisions = list(decisions)
        self.heard_texts = []
        self.spoken_texts = []

    async def heard(self, context, message):
        self.heard_texts.append(message.text_content)
        return self.decisions.pop(0)

    async def spoken(self, text):
        self.spoken_texts.append(text)
        return self.decisions.pop(0)


class Session:
    """Records what the tutor says outside the model, as the voice would."""

    def __init__(self):
        self.said = []
        self.interrupted = False

    def say(self, text, *, add_to_chat_ctx, allow_interruptions):
        self.said.append((text, add_to_chat_ctx, allow_interruptions))

        async def played():
            return SimpleNamespace(interrupted=self.interrupted)

        return played()


ALLOW = Decision(Action.ALLOW)
HAZARD = Decision(Action.REDIRECT, "adult_help")


async def hear(tutor, session, text="¿lo desenchufo?"):
    """One turn of the person. True when the tutor may answer it."""
    message = llm.ChatMessage(role="user", content=[text])
    with patch.object(Tutor, "session", new_callable=PropertyMock, return_value=session):
        try:
            await tutor.on_user_turn_completed(llm.ChatContext.empty(), message)
        except StopResponse:
            return False
    return True


async def stream(*chunks):
    for chunk in chunks:
        yield chunk


async def test_an_allowed_turn_goes_to_the_model_and_nothing_else_is_said():
    gates, session = ScriptedGates(ALLOW), Session()

    assert await hear(Tutor(language="es", gates=gates), session)
    assert session.said == []


@pytest.mark.parametrize(
    ("decision", "kept"),
    [
        (Decision(Action.REDIRECT, "redirect"), False),
        (Decision(Action.SUPPORT, "self_harm_support"), False),
        (Decision(Action.SUPPORT, "child_support"), False),
        (Decision(Action.UNAVAILABLE, "unavailable"), False),
        (Decision(Action.UNAVAILABLE, "context_limit"), False),
        # An offer stays in the conversation so the next answer can refer to it.
        (Decision(Action.REDIRECT, "clarify"), True),
        (HAZARD, True),
    ],
)
@pytest.mark.parametrize("language", ["es", "en"])
async def test_a_stopped_turn_gets_the_fixed_reply_and_never_reaches_the_model(
    decision, kept, language
):
    gates, session = ScriptedGates(decision), Session()

    assert not await hear(Tutor(language=language, gates=gates), session)
    assert session.said == [(REPLIES[decision.reply][language], kept, False)]


async def test_hazards_in_a_row_get_the_full_offer_and_then_shorter_reminders():
    gates, session = ScriptedGates(HAZARD, HAZARD, HAZARD, HAZARD, ALLOW, HAZARD), Session()
    tutor = Tutor(language="es", gates=gates)

    for _ in range(6):
        await hear(tutor, session)

    keys = ["adult_help", "adult_help_riddle", "adult_help_story", "adult_help_riddle"]
    assert [said[0] for said in session.said] == [
        REPLIES[key]["es"]
        for key in [*keys, "adult_help"]  # a safe turn starts over
    ]


async def test_a_reminder_that_was_not_heard_in_full_is_not_counted():
    gates, session = ScriptedGates(HAZARD, HAZARD), Session()
    session.interrupted = True
    tutor = Tutor(language="en", gates=gates)

    await hear(tutor, session)
    await hear(tutor, session)

    assert [said[0] for said in session.said] == [REPLIES["adult_help"]["en"]] * 2


async def test_a_reply_is_held_until_all_of_it_has_passed():
    gates = ScriptedGates(ALLOW)
    released = []

    async def model():
        yield "Muy "
        assert released == []  # nothing is spoken while the model is still writing
        yield llm.ChatChunk(id="1", delta=llm.ChoiceDelta(role="assistant", content="bien."))

    async for chunk in checked(model(), gates, "es"):
        released.append(chunk)

    assert gates.spoken_texts == ["Muy bien."]
    assert [getattr(chunk, "id", chunk) for chunk in released] == ["Muy ", "1"]


@pytest.mark.parametrize("key", ["redirect", "unavailable"])
async def test_a_reply_that_does_not_pass_is_replaced_whole(key):
    gates = ScriptedGates(Decision(Action.REDIRECT, key))

    spoken = [chunk async for chunk in checked(stream("algo ", "indebido"), gates, "en")]

    assert spoken == [REPLIES[key]["en"]]


def test_persona_falls_back_to_the_defaults():
    assert persona_from("Luna", "jugueton", " sé breve ") == Persona("Luna", "jugueton", "sé breve")
    # An unknown preset becomes calido, a blank name becomes Capri, a blank note None.
    assert persona_from("  ", "unknown", "   ") == Persona("Capri", "calido", None)
    assert persona_from(None, None, None) == Persona()


def test_persona_of_reads_the_dispatch_metadata():
    meta = '{"tutor_name": "Río", "tutor_persona": "aventurero", "tutor_persona_note": "fútbol"}'
    assert persona_of(meta) == Persona("Río", "aventurero", "fútbol")
    assert persona_of("") == Persona()
    assert persona_of("not json") == Persona()


def test_persona_notes_add_nothing_by_default_and_a_bounded_wish_with_a_note():
    assert persona_notes("es", Persona()) == ""
    assert "juguetón" in persona_notes("es", Persona(preset="jugueton"))
    wish = persona_notes("es", Persona(note="habla despacio"))
    assert "habla despacio" in wish and "seguridad" in wish


def test_a_named_persona_renames_the_tutor_and_the_default_leaves_the_prompt_as_written():
    default = Tutor(language="es", gates=ScriptedGates(ALLOW))
    assert default._base == INSTRUCTIONS["es"]
    assert "Capri" in default._greeting

    named = Tutor(
        language="es", gates=ScriptedGates(ALLOW), persona=Persona(name="Luna", preset="jugueton")
    )
    assert "Eres Luna" in named._base and "Capri" not in named._base
    assert "Luna" in named._greeting and "Capri" not in named._greeting
    # The tone line sits last, after the teaching, safety and context.
    assert named._base.rstrip().endswith("divertido.")
