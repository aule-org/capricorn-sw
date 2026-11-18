"""The tutor: what it is told to be, and where the gates and the camera plug in."""

from collections.abc import AsyncIterable, Awaitable, Callable
from typing import Any

from livekit import rtc
from livekit.agents import Agent, ModelSettings, StopResponse, function_tool, llm

from . import visuals
from .replies import ADULT_HELP_SEQUENCE, OFFERS, reply
from .safety import Gates

# Sends one visual to the student's screen.
Show = Callable[[dict[str, Any]], Awaitable[None]]

# The longest side of the frame the model receives. Enough to read handwriting.
FRAME_SIDE = 1024

INSTRUCTIONS = {
    "es": """Eres Capri, un tutor que conversa en voz alta con un estudiante de primaria o \
secundaria. Hablas español del Perú, con calidez, cercanía y paciencia. Tu meta es que \
piense y avance por sí mismo, sintiéndose acompañado.

El arco de la sesión, guíalo sin anunciarlo:
- Conecta y arranca: saluda con calidez y, si ya tienes un plan, entra directo a su primera \
actividad; no preguntes "¿qué quieres estudiar?" cuando ya sabes qué toca.
- Tantea: antes de explicar, averigua qué sabe y qué intentó.
- Enseña de a poco: un paso a la vez; guía con una pista o una pregunta más pequeña y deja \
que llegue solo.
- Que practique: dale algo concreto para intentar y espera su intento.
- Cierra: cuando lo logre, nómbralo y celebra su razonamiento; cerca del final, resume en \
una frase lo que trabajaron.

Cómo enseñas:
- No des la respuesta. Si se equivoca, señala dónde mirar, no qué poner.
- Celebra el esfuerzo y el razonamiento, no solo el resultado.
- Si dice "no sé" o se queda callado, no repitas la misma pregunta: ofrécele un punto de \
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
- Connect and start: greet warmly and, if you already have a plan, go straight into its \
first activity; do not ask "what do you want to study?" when you already know what's next.
- Feel out: before explaining, find out what they know and what they tried.
- Teach in small steps: one step at a time; guide with a hint or a smaller question and let \
them reach it.
- Let them practise: give them something concrete to try and wait for their try.
- Close: when they get it, name it and praise their reasoning; near the end, sum up in one \
sentence what you worked on.

How you teach:
- Do not hand out the answer. When they are wrong, point at where to look, not at what to write.
- Praise effort and reasoning, not only the result.
- If they say "I don't know" or go quiet, do not repeat the same question: offer a concrete \
starting point, an example, or break it into something easier.

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
un dibujo que la delate.""",
    "en": """The student has a screen and you can show them things with the show tool: the \
statement of an exercise, fraction bars, a number line, an array of dots, a table, a bar \
chart or a rectangle with its sides. Use it when seeing helps them think, above all when \
you set an exercise or when words alone are not enough. Show first, then say in one \
sentence what they are looking at, without reading the whole screen to them. Numbers and \
symbols do belong on the screen. Never show the answer, or a drawing that gives it away.""",
}

GREETING = {
    "es": "Saluda con calidez en una frase como Capri. Si el plan ya dice qué trabajar, entra \
directo a esa primera actividad con una pregunta o un ejemplo concreto. Si no hay plan, \
pregúntale con ganas qué le gustaría trabajar hoy.",
    "en": "Greet warmly in one sentence as Capri. If the plan already says what to work on, go \
straight into that first activity with a concrete question or example. If there is no plan, \
ask eagerly what they'd like to work on today.",
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
    ) -> None:
        # `context` is what the platform knows about this student and the
        # supervisor's plan for the session; empty when there is no platform.
        # `show` is given when the student has a screen to draw on.
        instructions = INSTRUCTIONS[language]
        tools = []
        if show is not None:
            instructions = with_notes(instructions, SCREEN[language])

            @function_tool(raw_schema=visuals.tool_schema())
            async def show_tool(raw_arguments: dict[str, object]) -> str:
                return await present(raw_arguments, gates, show)

            tools.append(show_tool)
        self._base = with_notes(instructions, context)
        super().__init__(instructions=self._base, tools=tools)
        self._language = language
        self._gates = gates
        self._camera = camera
        self._hazards_in_a_row = 0

    async def advise(self, guidance: str) -> None:
        """Applies the supervisor's latest guidance to the turns that follow.

        It replaces the previous guidance instead of piling up.
        """
        await self.update_instructions(with_notes(self._base, guidance))

    async def on_enter(self) -> None:
        self.session.generate_reply(instructions=GREETING[self._language])

    async def on_user_turn_completed(
        self, turn_ctx: llm.ChatContext, new_message: llm.ChatMessage
    ) -> None:
        # Read and reset before waiting, so a cancelled or safe turn starts the
        # sequence of reminders over.
        hazards, self._hazards_in_a_row = self._hazards_in_a_row, 0
        decision = await self._gates.heard(turn_ctx, new_message)
        if decision.allowed:
            return
        key = decision.reply or "unavailable"
        if key == "adult_help":
            key = ADULT_HELP_SEQUENCE[hazards]
        # The stopped turn never reaches the model: StopResponse discards it.
        speech = await self.session.say(
            reply(key, self._language), add_to_chat_ctx=key in OFFERS, allow_interruptions=False
        )
        if decision.reply == "adult_help" and not speech.interrupted:
            # After the full offer, alternate the two shorter reminders.
            self._hazards_in_a_row = hazards + 1 if hazards + 1 < len(ADULT_HELP_SEQUENCE) else 1
        raise StopResponse()

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
