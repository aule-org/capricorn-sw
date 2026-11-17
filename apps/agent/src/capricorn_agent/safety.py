"""The safety gates and who a session is for.

ARCHITECTURE.md, "Safety and privacy requirements": every child turn passes
content moderation and a physical-safety check, both fail closed, and a child's
session does not start unless both are configured.

No gate is implemented yet. Until they are, the only session the worker accepts
is an operator's: an adult on the team testing the tutor, marked as such by the
server that issued the room token.
"""

import asyncio
import json
from dataclasses import dataclass
from typing import Literal, Protocol

Audience = Literal["operator", "child"]

SAFE_REPLY = {
    "es": "De eso mejor conversa con un adulto de confianza. ¿Seguimos con lo que estábamos?",
    "en": "That is something to talk about with a grown-up you trust. Shall we get back to work?",
}


def audience_of(dispatch_metadata: str) -> Audience:
    """Who the session is for. Anything not explicitly an operator is a child."""
    try:
        metadata = json.loads(dispatch_metadata or "{}")
    except ValueError:
        return "child"
    if isinstance(metadata, dict) and metadata.get("audience") == "operator":
        return "operator"
    return "child"


class Gate(Protocol):
    async def allows(self, text: str) -> bool:
        """True only when the text is safe. Raising counts as not safe."""
        ...


@dataclass(frozen=True)
class Gates:
    """The two gates of a session. Either may be missing in an operator's session."""

    moderation: Gate | None = None
    physical_safety: Gate | None = None
    timeout_seconds: float = 1.5

    @property
    def complete(self) -> bool:
        return self.moderation is not None and self.physical_safety is not None

    def may_start(self, audience: Audience) -> bool:
        return audience == "operator" or self.complete

    async def allow_heard(self, text: str) -> bool:
        """What the person said, before the tutor answers it."""
        return await self._all(text, self.moderation, self.physical_safety)

    async def allow_spoken(self, text: str) -> bool:
        """What the tutor is about to say, before it is spoken."""
        return await self._all(text, self.moderation)

    @property
    def checks_spoken(self) -> bool:
        return self.moderation is not None

    async def _all(self, text: str, *gates: Gate | None) -> bool:
        configured = [gate for gate in gates if gate is not None]
        if not configured or not text.strip():
            return True
        try:
            async with asyncio.timeout(self.timeout_seconds):
                verdicts = await asyncio.gather(*(gate.allows(text) for gate in configured))
        except Exception:  # a gate that cannot answer is a gate that said no
            return False
        return all(verdict is True for verdict in verdicts)
