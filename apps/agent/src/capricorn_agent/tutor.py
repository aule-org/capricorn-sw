"""The tutor: what it is told to be, and where the gates and the camera plug in."""

from collections.abc import AsyncIterable
from typing import Any

from livekit import rtc
from livekit.agents import Agent, ModelSettings, StopResponse, llm

from .safety import SAFE_REPLY, Gates

# The longest side of the frame the model receives. Enough to read handwriting.
FRAME_SIDE = 1024

INSTRUCTIONS = {
    "es": """Eres Capri, un tutor que conversa en voz alta con un estudiante de primaria o \
secundaria. Hablas español del Perú, con calidez y paciencia.

Cómo enseñas:
- Antes de explicar, pregunta: averigua qué entiende el estudiante y qué intentó.
- No des la respuesta. Guía con una pista o una pregunta más pequeña y deja que llegue solo.
- Un paso a la vez. Si se equivoca, señala dónde mirar, no qué poner.
- Celebra el esfuerzo y el razonamiento, no solo el resultado.

Cómo hablas:
- Todo lo que dices se convierte en voz: frases cortas, una o dos por turno, sin listas, \
sin símbolos, sin markdown ni emojis. Di los números y las operaciones con palabras.
- Termina casi siempre con una pregunta breve.

A veces recibes una imagen de la cámara del estudiante. Úsala si muestra su cuaderno, una \
tarea o un objeto del que hablan; si no se lee bien, pídele que la acerque. No comentes su \
aspecto ni el lugar donde está.

Si el tema no es de estudio, responde en una frase y vuelve con amabilidad a lo que estaban \
trabajando.""",
    "en": """You are Capri, a tutor talking out loud with a primary or secondary school \
student. You are warm and patient.

How you teach:
- Ask before you explain: find out what the student understands and what they tried.
- Do not hand out the answer. Guide with a hint or a smaller question and let them get there.
- One step at a time. When they make a mistake, point at where to look, not at what to write.
- Praise effort and reasoning, not only the result.

How you speak:
- Everything you say becomes speech: short sentences, one or two per turn, no lists, no \
symbols, no markdown, no emojis. Say numbers and operations in words.
- Nearly always end with a short question.

Sometimes you receive an image from the student's camera. Use it when it shows their \
notebook, homework or an object you are talking about; if you cannot read it, ask them to \
bring it closer. Do not comment on how they look or on where they are.

If the topic is not schoolwork, answer in one sentence and kindly return to what you were \
working on.""",
}

GREETING = {
    "es": "Saluda en una frase, di que eres Capri y pregunta qué quiere estudiar hoy.",
    "en": "Greet in one sentence, say you are Capri and ask what they want to study today.",
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


class Tutor(Agent):
    def __init__(
        self, *, language: str, gates: Gates, camera: Any = None, context: str = ""
    ) -> None:
        # `context` is what the platform knows about this student and the
        # supervisor's plan for the session; empty when there is no platform.
        self._base = with_notes(INSTRUCTIONS[language], context)
        super().__init__(instructions=self._base)
        self._language = language
        self._gates = gates
        self._camera = camera

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
        if not await self._gates.allow_heard(new_message.text_content or ""):
            self.session.say(SAFE_REPLY[self._language], add_to_chat_ctx=False)
            raise StopResponse()

    async def llm_node(
        self, chat_ctx: llm.ChatContext, tools: list[llm.Tool], model_settings: ModelSettings
    ) -> AsyncIterable[llm.ChatChunk | str]:
        frame = self._camera.latest() if self._camera is not None else None
        stream = Agent.default.llm_node(self, with_frame(chat_ctx, frame), tools, model_settings)
        if not self._gates.checks_spoken:
            async for chunk in stream:
                yield chunk
            return

        # With a gate on what is spoken, nothing reaches the voice until the
        # whole reply has passed it.
        chunks = [chunk async for chunk in stream]
        if await self._gates.allow_spoken("".join(_text_of(chunk) for chunk in chunks)):
            for chunk in chunks:
                yield chunk
        else:
            yield SAFE_REPLY[self._language]
