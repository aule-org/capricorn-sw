"""A "repeat after me" in English: the sentence, the tune it should end with,
and whether the student's voice did that.

The tutor registers the sentence; the student's next spoken turn is their
try; ``prosody`` measures it and a rule, not the model, says what to tell
them. While a drill is open the last seconds of the student's audio are held
in memory, enough for one sentence; when it closes they are dropped. Nothing
is stored, logged or sent.
"""

from collections import deque
from typing import Any

import numpy as np
import numpy.typing as npt

from . import prosody

# One sentence said slowly, with the silence around it.
KEEP_SECONDS = 8.0
# Tries before the sentence is left for another time.
MAX_TRIES = 3
MAX_SENTENCE_CHARS = 120
# How the tune is drawn: the measured points, within an octave either way.
DRAWN_SEMITONES = 12.0

WAY = {"rising": "up", "falling": "down", "level": "flat"}
# The model tends to praise how the words sounded; that was not measured.
ONLY_THE_TUNE = " Only the tune was measured, not the sounds: say nothing about them."
TITLE = {"es": "Tu voz", "en": "Your voice"}

POSED = (
    "Registered. Say the sentence once, clearly, with its natural tune, then ask them to say it "
    "back. When they have said it, call check_phrase."
)


def _why(sentence: str, expected: prosody.Pattern) -> str:
    if not sentence.rstrip().endswith("?"):
        return "a statement"
    if expected == "rising":
        return "a yes/no question"
    return "a question with what, where, who or how, or a choice with or"


class Drill:
    def __init__(self) -> None:
        self.sentence: str | None = None
        self.expected: prosody.Pattern | None = None
        self.tries = 0
        self._frames: deque[npt.NDArray[np.int16]] = deque()
        self._rate = 0
        self._held = 0
        self._take: tuple[npt.NDArray[np.int16], int] | None = None

    @property
    def open(self) -> bool:
        return self.sentence is not None

    def pose(self, sentence: str) -> str:
        """Registers the sentence the student is about to say back."""
        sentence = " ".join(sentence.split())[:MAX_SENTENCE_CHARS]
        if not any(letter.isalpha() for letter in sentence):
            return "Not registered: give the English sentence they will repeat."
        self._drop()
        self.sentence = sentence
        self.expected = prosody.expected_pattern(sentence)
        self.tries = 0
        return POSED

    def hear(self, samples: npt.NDArray[np.int16], rate: int) -> None:
        """Keeps the last seconds of the student's audio while a drill is open."""
        if not self.open:
            return
        if rate != self._rate:
            self._frames.clear()
            self._held = 0
            self._rate = rate
        self._frames.append(samples)
        self._held += len(samples)
        while self._frames and self._held - len(self._frames[0]) >= KEEP_SECONDS * rate:
            self._held -= len(self._frames.popleft())

    def took(self) -> None:
        """The student finished a spoken turn: what was just heard is their try."""
        if not self.open or not self._frames:
            return
        self._take = (np.concatenate(list(self._frames)), self._rate)
        self._frames.clear()
        self._held = 0

    def check(self) -> tuple[str, prosody.Contour | None]:
        """What to tell the tutor about the student's last try, and its tune."""
        if self.sentence is None or self.expected is None:
            return "No sentence is registered: call pose_phrase first.", None
        take, self._take = self._take, None
        if take is None:
            return (
                "No spoken try was heard since the sentence was set. Ask them to say it aloud.",
                None,
            )
        contour = prosody.measure(take[0], take[1])
        if contour is None:
            return (
                "Too little voice to hear the tune. Ask them to say it again, a bit louder.",
                None,
            )
        self.tries += 1
        heard, expected = WAY[contour.pattern], WAY[self.expected]
        why = _why(self.sentence, self.expected)
        if contour.pattern == self.expected:
            self._close()
            return (
                f"Their voice went {heard} at the end, as {why} does. "
                "Say so warmly in a few words." + ONLY_THE_TUNE,
                contour,
            )
        if self.tries >= MAX_TRIES:
            self._close()
            return (
                f"Their voice went {heard} at the end; {why} ends going {expected}. That was the "
                "last try for now: say it once more yourself, kindly, and move on." + ONLY_THE_TUNE,
                contour,
            )
        return (
            f"Their voice went {heard} at the end; {why} ends going {expected}. Say it once more "
            "yourself, making the end a little bigger, and ask them to try again; then call "
            "check_phrase." + ONLY_THE_TUNE,
            contour,
        )

    def close(self) -> None:
        self._close()

    def _close(self) -> None:
        self.sentence = None
        self.expected = None
        self._drop()

    def _drop(self) -> None:
        self._frames.clear()
        self._held = 0
        self._take = None


def visual(contour: prosody.Contour, expected: prosody.Pattern, language: str) -> dict[str, Any]:
    """The tune as the screen draws it (kind ``contour``): the voiced stretches
    of the measured pitch, and which way the end should go."""
    segments: list[dict[str, Any]] = []
    for index, point in enumerate(contour.points):
        if point is None:
            continue
        value = round(max(-DRAWN_SEMITONES, min(DRAWN_SEMITONES, point)), 1)
        if segments and segments[-1]["from"] + len(segments[-1]["values"]) == index:
            segments[-1]["values"].append(value)
        else:
            segments.append({"from": index, "values": [value]})
    return {
        "kind": "contour",
        "title": TITLE.get(language, TITLE["en"]),
        "count": len(contour.points),
        "segments": segments,
        "expected": "falling" if expected == "level" else expected,
    }
