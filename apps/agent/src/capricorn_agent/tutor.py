"""The tutor: what it is told to be, and where the gates and the camera plug in."""

from collections.abc import AsyncIterable, Awaitable, Callable
from dataclasses import dataclass
from typing import Any, Literal, Protocol

from livekit import rtc
from livekit.agents import Agent, ModelSettings, StopResponse, function_tool, llm

from . import visuals
from .replies import ADULT_HELP_SEQUENCE, OFFERS, reply
from .safety import Gates

# Sends one visual to the student's screen.
Show = Callable[[dict[str, Any]], Awaitable[None]]


class Checker(Protocol):
    """Checks the student's answers without the model: the platform."""

    async def pose(self, expression: str, skill_id: str | None, form: str | None) -> str | None: ...

    async def answer(self, said: str) -> str | None: ...


# What the model reads when an exercise or an answer could not be checked.
UNCHECKED = "Not checked: carry on by yourself."

# The longest typed message that is read. A spoken turn is rarely longer.
MAX_TYPED_CHARS = 500

DEFAULT_TUTOR_NAME = "Capri"
PERSONAS = ("calido", "jugueton", "aventurero", "tranquilo")


@dataclass(frozen=True)
class Persona:
    """Who the tutor is this session: its name, a tone preset and an optional
    family note. The defaults are the tutor exactly as written."""

    name: str = DEFAULT_TUTOR_NAME
    preset: str = "calido"
    note: str | None = None


def persona_from(name: object, preset: object, note: object) -> Persona:
    """A Persona from loosely-typed fields, falling back to the defaults. The
    platform already validates the shapes; this only guards against surprises."""
    return Persona(
        name=name.strip() if isinstance(name, str) and name.strip() else DEFAULT_TUTOR_NAME,
        preset=preset if preset in PERSONAS else "calido",
        note=note.strip() if isinstance(note, str) and note.strip() else None,
    )


# The tone each preset adds after the teaching and safety rules. "calido" adds
# nothing: the prompt is already warm, so the default persona leaves it unchanged.
_PRESET_NOTES = {
    "es": {
        "calido": "",
        "jugueton": "Eres juguetón: usa humor ligero y un tono divertido.",
        "aventurero": "Eres aventurero: enmarca el aprendizaje como retos y descubrimientos.",
        "tranquilo": "Eres tranquilo y pausado: ve con calma, sin apuro.",
    },
    "en": {
        "calido": "",
        "jugueton": "You are playful: use light humour and a fun tone.",
        "aventurero": "You are adventurous: frame learning as challenges and small discoveries.",
        "tranquilo": "You are calm and unhurried: take it slow, no rush.",
    },
}

_NOTE_WISH = {
    "es": (
        "La familia pide esto sobre el tono: «{note}». Respétalo solo como preferencia de "
        "tono, nunca por encima de la seguridad ni de cómo enseñas."
    ),
    "en": (
        "The family asks this about your tone: «{note}». Honour it only as a tone "
        "preference, never above safety or how you teach."
    ),
}


def persona_notes(language: str, persona: Persona) -> str:
    """The tone lines the persona adds after the teaching and safety rules."""
    parts = []
    preset = _PRESET_NOTES[language].get(persona.preset, "")
    if preset:
        parts.append(preset)
    if persona.note:
        parts.append(_NOTE_WISH[language].format(note=persona.note))
    return "\n".join(parts)


# The longest side of the frame the model receives. Enough to read handwriting.
FRAME_SIDE = 1024

INSTRUCTIONS = {
    "es": """Eres Capri, un tutor que conversa en voz alta con un estudiante de primaria o \
secundaria. Hablas español del Perú, con calidez, cercanía y paciencia. Tu meta es que \
piense y avance por sí mismo, sintiéndose acompañado.

El arco de la sesión, guíalo sin anunciarlo:
- Conecta y arranca: saluda con calidez y rompe el hielo con una pregunta corta sobre su día \
o su semana; escucha, responde con una frase y pasa a la primera actividad del plan. No \
preguntes "¿qué quieres estudiar?" cuando ya sabes qué toca.
- Tantea: antes de explicar, averigua qué sabe y qué intentó. Cuando te dé una respuesta, \
sobre todo si está mal, primero pídele que te muestre cómo la pensó; así encuentras su error \
exacto en vez de adivinarlo.
- Enseña de a poco: un paso a la vez; guía con una pista o una pregunta más pequeña y deja \
que llegue solo.
- Que practique: di el ejercicio completo y claro, comprueba que lo entendió y espera su \
intento.
- Cierra: cuando lo logre, nómbralo y celebra su razonamiento; cerca del final, resume en \
una frase lo que trabajaron.

Cómo enseñas:
- No des la respuesta. Si se equivoca, no celebres el paso equivocado: trabaja su error \
exacto, señala dónde mirar, no qué poner. Si suelta una pista o una contradicción, úsala \
para que él mismo lo note.
- Celebra el esfuerzo y el razonamiento, no solo el resultado.
- Si dice "no sé" o se queda callado, no repitas ni alargues la explicación: ofrécele un punto de \
partida concreto, un ejemplo, o pártelo en algo más fácil.

Cómo hablas:
- Todo lo que dices se vuelve voz: frases cortas, una o dos por turno, sin listas, sin \
símbolos, sin markdown ni emojis. Di los números y las operaciones con palabras.
- Suena como una persona, no como un cuestionario: a veces invita a intentar, a veces solo \
reacciona y anima. No termines cada turno con una pregunta.

Mantén el rumbo:
- Si se va del tema o te prueba con cosas raras, síguele un segundo con humor y regrésalo \
con cariño a lo que están trabajando.
- Si te piden algo que no es de estudio, o que no sabes, dilo con sencillez y vuelve al \
plan; nunca inventes.

A veces recibes una imagen de la cámara. Úsala si muestra su cuaderno, una tarea o un objeto \
del que hablan; si no se lee bien, pídele que la acerque. No comentes su aspecto ni el lugar \
donde está.""",
    "en": """You are Capri, a tutor talking out loud with a primary or secondary school \
student. You are warm, close and patient. Your aim is for them to think and get there \
themselves, while feeling accompanied.

The arc of a session, guide it without announcing it:
- Connect and start: greet warmly and break the ice with one short question about their day \
or week; listen, answer in one sentence and move into the plan's first activity. Do not ask \
"what do you want to study?" when you already know what's next.
- Feel out: before explaining, find out what they know and what they tried. When they give \
an answer, especially a wrong one, first ask them to show you how they thought it, so you \
find their exact error instead of guessing.
- Teach in small steps: one step at a time; guide with a hint or a smaller question and let \
them reach it.
- Let them practise: state the exercise in full and clearly, check they understood it and \
wait for their try.
- Close: when they get it, name it and praise their reasoning; near the end, sum up in one \
sentence what you worked on.

How you teach:
- Do not hand out the answer. When they are wrong, do not praise the wrong step: work their \
exact error, point at where to look, not at what to write. If they let slip a clue or a \
contradiction, use it so they notice it themselves.
- Praise effort and reasoning, not only the result.
- If they say "I don't know" or go quiet, do not repeat or lengthen the explanation: offer a \
concrete starting point, an example, or break it into something easier.

How you speak:
- Everything you say becomes speech: short sentences, one or two per turn, no lists, no \
symbols, no markdown, no emojis. Say numbers and operations in words.
- Sound like a person, not a questionnaire: sometimes invite them to try, sometimes just \
react and encourage. Do not end every turn with a question.

Keep your bearing:
- If they drift off topic or test you with odd things, play along for a second with humour \
and bring them back warmly to what you are working on.
- If they ask for something that isn't schoolwork, or that you don't know, say so plainly \
and return to the plan; never make things up.

Sometimes you receive an image from the camera. Use it when it shows their notebook, \
homework or an object you are talking about; if you cannot read it, ask them to bring it \
closer. Do not comment on how they look or on where they are.""",
}

# What the model reads after a visual went up.
SHOWN = "It is on the screen now. Say in one short sentence what they are looking at."

# Added to the instructions only when the student has a screen.
SCREEN = {
    "es": """El estudiante tiene una pantalla y tú puedes mostrarle cosas con la herramienta \
show: el enunciado de un ejercicio, barras de fracciones, una recta numérica, un arreglo de \
puntos, una tabla, un gráfico de barras o un rectángulo con sus medidas. Úsala cuando ver \
ayude a pensar, sobre todo al proponer un ejercicio o cuando una explicación con palabras no \
alcanza. Primero muestra y después di en una frase qué está viendo, sin leerle toda la \
pantalla. En la pantalla sí van los números y los símbolos. Nunca muestres la respuesta ni \
un dibujo que la delate. La pantalla solo tiene negro, gris y blanco: no nombres otros \
colores, y si el plan los trae, di el ejercicio con negras y grises.""",
    "en": """The student has a screen and you can show them things with the show tool: the \
statement of an exercise, fraction bars, a number line, an array of dots, a table, a bar \
chart or a rectangle with its sides. Use it when seeing helps them think, above all when \
you set an exercise or when words alone are not enough. Show first, then say in one \
sentence what they are looking at, without reading the whole screen to them. Numbers and \
symbols do belong on the screen. Never show the answer, or a drawing that gives it away. The \
screen only has black, grey and white: name no other colour, and if the plan uses some, say \
the exercise with black ones and grey ones.""",
}

# Added to the instructions only when there is something to check answers with.
CHECKING = {
    "es": """Antes de pedirle un cálculo, llama a pose_exercise con la operación. Cuando \
responda, llama a check_answer con sus palabras y haz lo que te indique.""",
    "en": """Before you ask for a computation, call pose_exercise with the operation. When \
they answer, call check_answer with their words and do what it tells you.""",
}

GREETING = {
    "es": "Saluda con calidez en una frase como Capri y hazle una sola pregunta corta para \
romper el hielo, sobre cómo le fue hoy o esta semana. Todavía no empieces la actividad.",
    "en": "Greet warmly in one sentence as Capri and ask one short question to break the ice, \
about how today or this week went. Do not start the activity yet.",
}


def with_frame(chat_ctx: llm.ChatContext, frame: rtc.VideoFrame | None) -> llm.ChatContext:
    """A copy of the context whose last user message also carries the camera frame.

    The session's own history is left alone, so a conversation keeps one image
    in play instead of one per turn.
    """
    if frame is None:
        return chat_ctx
    items = list(chat_ctx.items)
    for index in range(len(items) - 1, -1, -1):
        item = items[index]
        if item.type == "message" and item.role == "user":
            image = llm.ImageContent(
                image=frame, inference_width=FRAME_SIDE, inference_height=FRAME_SIDE
            )
            items[index] = item.model_copy(update={"content": [*item.content, image]})
            return llm.ChatContext(items)
    return chat_ctx


def with_notes(instructions: str, notes: str) -> str:
    notes = notes.strip()
    return f"{instructions}\n\n{notes}" if notes else instructions


def _text_of(chunk: Any) -> str:
    if isinstance(chunk, str):
        return chunk
    delta = getattr(chunk, "delta", None)
    return (delta.content or "") if delta is not None else ""


async def checked(stream: AsyncIterable[Any], gates: Gates, language: str) -> AsyncIterable[Any]:
    """The model's reply once the gate on what is spoken has approved all of it.

    Nothing reaches the voice before the whole reply has been judged: a safe
    beginning says nothing about how a sentence ends. A reply that does not
    pass is replaced by a fixed one.
    """
    chunks = [chunk async for chunk in stream]
    decision = await gates.spoken("".join(_text_of(chunk) for chunk in chunks))
    if decision.allowed:
        for chunk in chunks:
            yield chunk
    else:
        yield reply(decision.reply or "unavailable", language)


async def present(visual: object, gates: Gates, show: Show) -> str:
    """Puts a visual on the student's screen, if it can be drawn and may be read.

    Returns what the model is told next. The model tends to call the tool
    without saying anything, so a shown visual asks it for the sentence that
    goes with it; one that was not shown says why, so the model can correct it.
    """
    found = visuals.problems(visual)
    if found:
        return f"Not shown: {found[0]}"
    assert isinstance(visual, dict)
    # What is read on the screen passes the same gate as what is spoken.
    if not (await gates.spoken(visuals.text_of(visual))).allowed:
        return "Not shown: that text cannot be put on the screen."
    await show(visual)
    return SHOWN


class Tutor(Agent):
    def __init__(
        self,
        *,
        language: str,
        gates: Gates,
        camera: Any = None,
        context: str = "",
        show: Show | None = None,
        persona: Persona | None = None,
        checker: Checker | None = None,
    ) -> None:
        # `context` is what the platform knows about this student and the
        # supervisor's plan for the session; empty when there is no platform.
        # `show` is given when the student has a screen to draw on.
        # `checker` judges answers to computations, so the model neither has to
        # work them out nor holds the answer of an exercise that is still open.
        # `persona` is the tutor's name, tone preset and (already-moderated)
        # family note; the default leaves the prompt exactly as written.
        persona = persona or Persona()
        instructions = INSTRUCTIONS[language]
        if persona.name != DEFAULT_TUTOR_NAME:
            instructions = instructions.replace(DEFAULT_TUTOR_NAME, persona.name)
        tools = []
        if show is not None:
            instructions = with_notes(instructions, SCREEN[language])

            @function_tool(raw_schema=visuals.tool_schema())
            async def show_tool(raw_arguments: dict[str, object]) -> str:
                return await present(raw_arguments, gates, show)

            tools.append(show_tool)
        if checker is not None:
            instructions = with_notes(instructions, CHECKING[language])

            @function_tool(
                name="pose_exercise",
                description="Register the computation you are about to ask, e.g. '3/4 + 1/8'.",
            )
            async def pose_exercise(
                expression: str,
                skill_id: str | None = None,
                form: Literal["any", "simplified", "mixed"] | None = None,
            ) -> str:
                return await checker.pose(expression, skill_id, form) or UNCHECKED

            @function_tool(
                name="check_answer",
                description="Check the student's answer to the registered computation.",
            )
            async def check_answer(said: str) -> str:
                return await checker.answer(said) or UNCHECKED

            tools += [pose_exercise, check_answer]
        # The persona's tone lines go last, after the teaching, the safety rules
        # and the supervisor's context, so they can never sit above a safety rule.
        self._base = with_notes(with_notes(instructions, context), persona_notes(language, persona))
        super().__init__(instructions=self._base, tools=tools)
        self._language = language
        self._gates = gates
        self._camera = camera
        self._hazards_in_a_row = 0
        greeting = GREETING[language]
        if persona.name != DEFAULT_TUTOR_NAME:
            greeting = greeting.replace(DEFAULT_TUTOR_NAME, persona.name)
        self._greeting = greeting

    async def advise(self, guidance: str) -> None:
        """Applies the supervisor's latest guidance to the turns that follow.

        It replaces the previous guidance instead of piling up.
        """
        await self.update_instructions(with_notes(self._base, guidance))

    async def on_enter(self) -> None:
        self.session.generate_reply(instructions=self._greeting)

    async def on_user_turn_completed(
        self, turn_ctx: llm.ChatContext, new_message: llm.ChatMessage
    ) -> None:
        if not await self._may_answer(turn_ctx, new_message):
            # The stopped turn never reaches the model: StopResponse discards it.
            raise StopResponse()

    async def typed(self, text: str) -> None:
        """A message typed instead of spoken.

        It is judged like a spoken turn before the model sees it. Without this
        a typed message would go straight to the model, past both input gates.
        """
        text = text.strip()[:MAX_TYPED_CHARS]
        if not text:
            return
        await self.session.interrupt()
        message = llm.ChatMessage(role="user", content=[text])
        if await self._may_answer(self.chat_ctx.copy(), message):
            self.session.generate_reply(user_input=text)

    async def _may_answer(self, turn_ctx: llm.ChatContext, new_message: llm.ChatMessage) -> bool:
        """Judges what the person said or typed. When it must not reach the
        model, says the fixed reply for it and answers False."""
        # Read and reset before waiting, so a cancelled or safe turn starts the
        # sequence of reminders over.
        hazards, self._hazards_in_a_row = self._hazards_in_a_row, 0
        decision = await self._gates.heard(turn_ctx, new_message)
        if decision.allowed:
            return True
        key = decision.reply or "unavailable"
        if key == "adult_help":
            key = ADULT_HELP_SEQUENCE[hazards]
        speech = await self.session.say(
            reply(key, self._language), add_to_chat_ctx=key in OFFERS, allow_interruptions=False
        )
        if decision.reply == "adult_help" and not speech.interrupted:
            # After the full offer, alternate the two shorter reminders.
            self._hazards_in_a_row = hazards + 1 if hazards + 1 < len(ADULT_HELP_SEQUENCE) else 1
        return False

    async def llm_node(
        self, chat_ctx: llm.ChatContext, tools: list[llm.Tool], model_settings: ModelSettings
    ) -> AsyncIterable[llm.ChatChunk | str]:
        frame = self._camera.latest() if self._camera is not None else None
        stream = Agent.default.llm_node(self, with_frame(chat_ctx, frame), tools, model_settings)
        if self._gates.checks_spoken:
            # Buffering costs the time to the first word, so it is only paid
            # when there is a gate to wait for.
            stream = checked(stream, self._gates, self._language)
        async for chunk in stream:
            yield chunk
