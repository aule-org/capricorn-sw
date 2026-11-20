"""What the session engine wrote for the tutor to say.

The engine decides a session: the question to ask, the hint that is due, what
to say when an answer is right or wrong, when to stop. Its words were written
and read by people, so they are said as they are: no model writes them, no
model is asked. The model only speaks when the student leaves the script, to
ask something or to talk.
"""

from dataclasses import dataclass
from typing import Any, Protocol

from . import visuals

# The longest line that is said. A line of the script is a sentence or two.
MAX_LINE_CHARS = 1200
MAX_LINES = 8


@dataclass(frozen=True)
class Line:
    """One thing the tutor says, and what goes on the screen with it."""

    say: str
    show: dict[str, Any] | None = None


@dataclass(frozen=True)
class Reply:
    """The engine's answer to what the student said.

    ``lines`` are said as written. With ``converse`` the model then answers the
    student within those rules (the student asked something); without it the
    turn is over and the model is not called.
    """

    lines: tuple[Line, ...] = ()
    converse: str | None = None


class Flow(Protocol):
    """The engine, as the tutor sees it: the platform."""

    def opening(self) -> tuple[Line, ...]: ...

    async def said(self, text: str) -> Reply | None: ...


def lines_from(value: object) -> tuple[Line, ...]:
    """The lines in something the platform sent. Anything malformed is left out:
    a broken line is never said and never stops the others."""
    if not isinstance(value, list):
        return ()
    lines = []
    for item in value[:MAX_LINES]:
        if not isinstance(item, dict):
            continue
        say = item.get("say")
        if not isinstance(say, str) or not say.strip() or len(say) > MAX_LINE_CHARS:
            continue
        show = item.get("show")
        drawable = isinstance(show, dict) and not visuals.problems(show)
        lines.append(Line(say=say.strip(), show=show if drawable else None))
    return tuple(lines)


def reply_from(value: object) -> Reply | None:
    """The reply in the platform's answer, or None when it has nothing to say
    and the turn is the model's."""
    if not isinstance(value, dict):
        return None
    lines = lines_from(value.get("lines"))
    converse = value.get("converse")
    rules = converse.strip() if isinstance(converse, str) and converse.strip() else None
    if not lines and rules is None:
        return None
    return Reply(lines=lines, converse=rules)
