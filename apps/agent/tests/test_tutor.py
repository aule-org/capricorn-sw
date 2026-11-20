import asyncio
import time
from types import SimpleNamespace
from unittest.mock import PropertyMock, patch

import pytest
from livekit import rtc
from livekit.agents import StopResponse, llm

from aule_agent.greetings import GREETINGS, greeting
from aule_agent.moderation import Action, Decision
from aule_agent.replies import ADULT_HELP_SEQUENCE, REPLIES, reply
from aule_agent.script import Line, Reply
from aule_agent.tutor import (
    INSTRUCTIONS,
    MAX_TYPED_CHARS,
    UNCHECKED,
    Persona,
    Tutor,
    checked,
    for_the_student,
    persona_from,
    persona_notes,
    shows_something,
    with_frame,
)
from aule_agent.worker import GOODBYE, language_of, persona_of


@pytest.fixture(autouse=True)
def no_waiting_for_a_missing_reply(monkeypatch):
    monkeypatch.setattr("aule_agent.tutor.NO_REPLY_SECONDS", 0)


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

    def interrupt(self):
        self.interrupts = getattr(self, "interrupts", 0) + 1

        async def done():
            return None

        return done()

    def generate_reply(self, *, user_input):
        self.replies_to = [*getattr(self, "replies_to", []), user_input]

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
    # An unknown preset becomes calido, a blank name becomes Aule, a blank note None.
    assert persona_from("  ", "unknown", "   ") == Persona("Aule", "calido", None)
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
    assert default._tutor_name == "Aule"

    named = Tutor(
        language="es", gates=ScriptedGates(ALLOW), persona=Persona(name="Luna", preset="jugueton")
    )
    assert "Eres Luna" in named._base and "Aule" not in named._base
    assert named._tutor_name == "Luna"
    # The tone line sits last, after the teaching, safety and context.
    assert named._base.rstrip().endswith("divertido.")


async def type_to(tutor, session, text):
    empty = llm.ChatContext.empty()
    with (
        patch.object(Tutor, "session", new_callable=PropertyMock, return_value=session),
        patch.object(Tutor, "chat_ctx", new_callable=PropertyMock, return_value=empty),
    ):
        await tutor.typed(text)


async def test_a_typed_message_is_judged_like_a_spoken_one_before_the_model_sees_it():
    gates, session = ScriptedGates(ALLOW), Session()

    await type_to(Tutor(language="es", gates=gates), session, "  ¿cuánto es 3/4 + 1/8?  ")

    assert gates.heard_texts == ["¿cuánto es 3/4 + 1/8?"]
    assert session.replies_to == ["¿cuánto es 3/4 + 1/8?"]
    assert session.interrupts == 1
    assert session.said == []


@pytest.mark.parametrize("decision", [HAZARD, Decision(Action.REDIRECT, "redirect")])
async def test_a_typed_message_the_gates_stop_never_reaches_the_model(decision):
    gates, session = ScriptedGates(decision), Session()

    await type_to(Tutor(language="es", gates=gates), session, "¿lo desenchufo?")

    assert not hasattr(session, "replies_to")
    key = ADULT_HELP_SEQUENCE[0] if decision is HAZARD else "redirect"
    assert [said[0] for said in session.said] == [reply(key, "es")]


async def test_an_empty_typed_message_is_ignored_and_a_long_one_is_cut():
    gates, session = ScriptedGates(ALLOW), Session()
    tutor = Tutor(language="es", gates=gates)

    await type_to(tutor, session, "   ")
    assert gates.heard_texts == []
    assert not hasattr(session, "interrupts")

    await type_to(tutor, session, "a" * 2000)
    assert [len(text) for text in gates.heard_texts] == [MAX_TYPED_CHARS]
    assert [len(text) for text in session.replies_to] == [MAX_TYPED_CHARS]


class ScriptedChecker:
    """Stands in for the platform's exercise checker."""

    def __init__(self, posed=None, judged=None):
        self.posed, self.judged, self.calls = posed, judged, []

    async def pose(self, expression, skill_id, form):
        self.calls.append(("pose", expression, skill_id, form))
        return self.posed

    async def answer(self, said):
        self.calls.append(("answer", said))
        return self.judged


def tool(tutor, name):
    return next(tool for tool in tutor.tools if tool.info.name == name)


def test_the_checking_tools_exist_only_with_something_to_check_with():
    plain = Tutor(language="es", gates=ScriptedGates())
    checking = Tutor(language="es", gates=ScriptedGates(), checker=ScriptedChecker())

    assert [tool.info.name for tool in plain.tools] == []
    assert "pose_exercise" not in plain.instructions
    assert [tool.info.name for tool in checking.tools] == ["pose_exercise", "check_answer"]
    assert "pose_exercise" in checking.instructions


async def test_the_model_is_told_what_the_checker_said_and_nothing_else():
    checker = ScriptedChecker(posed="Ask it and wait.", judged="Give one hint about the sign.")
    tutor = Tutor(language="es", gates=ScriptedGates(), checker=checker)

    assert await tool(tutor, "pose_exercise")({"expression": "3/4 + 1/8"}) == "Ask it and wait."
    assert await tool(tutor, "check_answer")({"said": "me da cuatro doceavos"}) == (
        "Give one hint about the sign."
    )
    assert checker.calls == [
        ("pose", "3/4 + 1/8", None, None),
        ("answer", "me da cuatro doceavos"),
    ]


async def test_an_exercise_that_cannot_be_checked_leaves_the_tutor_on_its_own():
    tutor = Tutor(language="en", gates=ScriptedGates(), checker=ScriptedChecker())

    assert await tool(tutor, "pose_exercise")({"expression": "which is bigger?"}) == UNCHECKED
    assert await tool(tutor, "check_answer")({"said": "the red one"}) == UNCHECKED


def nullable(schema):
    """Every place a tool schema declares a value that may be null."""
    if isinstance(schema, dict):
        kind = schema.get("type")
        here = kind == "null" or (isinstance(kind, list) and "null" in kind)
        return here or any(nullable(value) for value in schema.values())
    return isinstance(schema, list) and any(nullable(item) for item in schema)


def test_no_tool_declares_a_nullable_argument_which_the_model_api_refuses_with_an_enum():
    tutor = Tutor(
        language="es", gates=ScriptedGates(), checker=ScriptedChecker(), show=lambda visual: None
    )

    assert len(tutor.tools) == 3
    for each in tutor.tools:
        assert not nullable(each.info.raw_schema), each.info.name


async def test_a_checking_call_without_its_text_is_not_sent_to_the_platform():
    checker = ScriptedChecker(posed="x", judged="y")
    tutor = Tutor(language="es", gates=ScriptedGates(), checker=checker)

    assert await tool(tutor, "pose_exercise")({"skill_id": "a.b"}) == UNCHECKED
    assert await tool(tutor, "check_answer")({"said": 7}) == UNCHECKED
    assert checker.calls == []


async def test_an_accepted_turn_without_a_reply_is_reported(caplog):
    tutor = Tutor(language="es", gates=ScriptedGates(ALLOW))
    session = Session()
    session.agent_state = "listening"

    with caplog.at_level("WARNING", logger="aule.agent.tutor"):
        assert await hear(tutor, session)
        with patch.object(Tutor, "session", new_callable=PropertyMock, return_value=session):
            await asyncio.gather(*tutor._watches)

    assert [record.message for record in caplog.records] == ["no reply after an accepted turn"]
    assert caplog.records[0].model_answered is False


async def test_an_answered_turn_is_not_reported(caplog):
    tutor = Tutor(language="es", gates=ScriptedGates(ALLOW))
    session = Session()
    session.agent_state = "speaking"

    with caplog.at_level("WARNING", logger="aule.agent.tutor"):
        assert await hear(tutor, session)
        tutor._model_done_at = time.monotonic()
        with patch.object(Tutor, "session", new_callable=PropertyMock, return_value=session):
            await asyncio.gather(*tutor._watches)

    assert caplog.records == []


async def test_a_late_plan_joins_the_student_context_and_keeps_the_guidance():
    tutor = Tutor(
        language="es",
        gates=ScriptedGates(),
        context="The student is Liam.",
        persona=persona_from("Aule", "jugueton", None),
    )
    seen: list[str] = []

    async def update(instructions):
        seen.append(instructions)

    with patch.object(Tutor, "update_instructions", side_effect=update):
        await tutor.advise("Slow down.")
        await tutor.plan("Plan: fractions.")

    final = seen[-1]
    assert final.index("The student is Liam.") < final.index("Plan: fractions.")
    assert final.index("Plan: fractions.") < final.index("divertido.") < final.index("Slow down.")


async def test_only_the_paragraph_for_the_student_is_spoken_and_judged():
    gates = ScriptedGates(ALLOW)
    call = llm.ChatChunk(
        id="tool",
        delta=llm.ChoiceDelta(
            role="assistant",
            tool_calls=[llm.FunctionToolCall(name="show", arguments="{}", call_id="c1")],
        ),
    )
    model = stream(
        "Esto no es un cálculo, así que no lo registro.\n\n", "Cuéntame cómo lo pensaste.", call
    )

    released = [chunk async for chunk in checked(model, gates, "es")]

    assert gates.spoken_texts == ["Cuéntame cómo lo pensaste."]
    assert released == ["Cuéntame cómo lo pensaste.", call]


def test_a_reply_of_one_paragraph_is_left_as_written():
    assert for_the_student("Muy bien.\nSigamos.") == "Muy bien.\nSigamos."
    assert for_the_student("Pienso.\n\n\n  Hola.  ") == "Hola."


async def enter(tutor, session):
    with patch.object(Tutor, "session", new_callable=PropertyMock, return_value=session):
        await tutor.on_enter()


async def test_the_session_opens_with_a_written_greeting_and_no_model():
    tutor = Tutor(language="es", gates=ScriptedGates(ALLOW), student_name="Liam", session_number=0)
    session = Session()

    await enter(tutor, session)

    assert session.said == [
        ("¡Hola, Liam! Soy Aule, qué gusto verte. Cuéntame, ¿cómo te fue hoy?", True, True)
    ]
    assert not hasattr(session, "replies_to")


async def test_a_name_the_gate_stops_is_left_out_of_the_greeting():
    stopped = ScriptedGates(Decision(Action.REDIRECT, "redirect"))
    tutor = Tutor(language="en", gates=stopped, student_name="x", session_number=1)
    session = Session()

    await enter(tutor, session)

    assert session.said == [("Hi! Aule here. What was the best part of your day?", True, True)]


def test_greetings_are_taken_in_turn_carry_the_tutors_name_and_exist_in_every_language():
    for language in ("es", "en"):
        said = [
            greeting(language, "Luna", "Liam", number) for number in range(len(GREETINGS[language]))
        ]
        assert len(set(said)) == len(GREETINGS[language])
        assert all("Luna" in text and "Liam" in text and "{" not in text for text in said)
        assert greeting(language, "Luna", None, 0) != said[0]
        assert greeting(language, "Luna", "Liam", len(said)) == said[0]
        assert greeting(language, "Luna") in [
            greeting(language, "Luna", None, number) for number in range(len(said))
        ]
    assert len(GREETINGS["es"]) == len(GREETINGS["en"]) >= 10


class ScriptedFlow:
    """Stands in for the session engine."""

    def __init__(self, opening=(), *replies):
        self._opening, self.replies, self.heard = tuple(opening), list(replies), []

    def opening(self):
        return self._opening

    async def said(self, text):
        self.heard.append(text)
        return self.replies.pop(0) if self.replies else None


async def scripted_turn(tutor, session, text="tres cuartos"):
    """One turn with the engine. True when the model is left to answer."""
    updates = {"instructions": [], "context": []}

    async def instructions(value):
        updates["instructions"].append(value)

    async def context(value):
        updates["context"].append(value)

    message = llm.ChatMessage(role="user", content=[text])
    with (
        patch.object(Tutor, "session", new_callable=PropertyMock, return_value=session),
        patch.object(Tutor, "update_instructions", side_effect=instructions),
        patch.object(Tutor, "update_chat_ctx", side_effect=context),
    ):
        try:
            await tutor.on_user_turn_completed(llm.ChatContext.empty(), message)
        except StopResponse:
            return False, updates
    return True, updates


async def test_the_session_opens_with_the_engines_words_and_no_model():
    shown = []

    async def show(visual):
        shown.append(visual)

    drawing = {"kind": "dot_array", "rows": 2, "columns": 3}
    flow = ScriptedFlow([Line("¡Hola, Liam!"), Line("Mira el círculo.", show=drawing)])
    tutor, session = Tutor(language="es", gates=ScriptedGates(), flow=flow, show=show), Session()

    with patch.object(Tutor, "session", new_callable=PropertyMock, return_value=session):
        await tutor.on_enter()

    assert session.said == [("¡Hola, Liam!", True, True), ("Mira el círculo.", True, True)]
    assert shown == [drawing]
    assert not hasattr(session, "replies_to")


async def test_a_hint_from_the_engine_is_said_as_written_and_the_model_is_not_called():
    hint = Reply(lines=(Line("¿Un medio y un tercio son pedazos del mismo tamaño?"),))
    flow = ScriptedFlow([Line("hola")], hint)
    tutor, session = Tutor(language="es", gates=ScriptedGates(ALLOW), flow=flow), Session()

    to_the_model, updates = await scripted_turn(tutor, session, "cinco sextos")

    assert not to_the_model
    assert flow.heard == ["cinco sextos"]
    assert session.said == [("¿Un medio y un tercio son pedazos del mismo tamaño?", True, True)]
    # What the student said stays in the conversation.
    assert updates["context"][0].items[-1].text_content == "cinco sextos"


async def test_when_the_student_leaves_the_script_the_model_answers_within_the_rules():
    flow = ScriptedFlow([Line("hola")], Reply(converse="No hints: this is a measurement."), None)
    tutor, session = Tutor(language="es", gates=ScriptedGates(ALLOW, ALLOW), flow=flow), Session()

    asked, first = await scripted_turn(tutor, session, "¿qué es un denominador?")
    off_script, second = await scripted_turn(tutor, session, "tengo hambre")

    assert asked and off_script and session.said == []
    assert first["instructions"][-1].endswith("No hints: this is a measurement.")
    # The rules of one moment do not outlive it.
    assert "No hints" not in second["instructions"][-1]


async def test_a_turn_the_gates_stop_never_reaches_the_engine():
    flow = ScriptedFlow([Line("hola")], Reply(lines=(Line("x"),)))
    tutor, session = Tutor(language="es", gates=ScriptedGates(HAZARD), flow=flow), Session()

    to_the_model, _ = await scripted_turn(tutor, session)

    assert not to_the_model and flow.heard == []


async def test_without_an_engine_the_turn_is_the_models_as_before():
    tutor, session = Tutor(language="es", gates=ScriptedGates(ALLOW)), Session()

    to_the_model, updates = await scripted_turn(tutor, session)

    assert to_the_model and updates == {"instructions": [], "context": []}


def conversation(*turns):
    chat = llm.ChatContext.empty()
    for role, text in turns:
        chat.add_message(role=role, content=text)
    return chat


@pytest.mark.parametrize(
    "turns",
    [
        [("assistant", "¿Cuánto te salió?"), ("user", "Mira, aquí está mi cuaderno.")],
        [("assistant", "Muéstrame cómo lo hiciste."), ("user", "Ya.")],
        [("assistant", "Acércalo un poco a la cámara."), ("user", "¿Así?")],
        [("assistant", "How did it go?"), ("user", "Look, I wrote it here.")],
        [("user", "Te muestro mi tarea.")],
    ],
)
def test_the_camera_is_looked_at_when_work_is_shown_or_asked_for(turns):
    assert shows_something(conversation(*turns))


@pytest.mark.parametrize(
    "turns",
    [
        [("assistant", "¿Cómo te fue hoy?"), ("user", "Bien, jugué fútbol.")],
        [("assistant", "¿Cuántas canicas sombreadas hay?"), ("user", "Creo que tres.")],
        [("user", "Mira mi cuaderno."), ("assistant", "Muy bien."), ("user", "¿Y ahora?")],
        [],
    ],
)
def test_the_camera_is_left_alone_in_ordinary_talk(turns):
    assert not shows_something(conversation(*turns))
