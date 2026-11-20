"""How a session felt to the student: how long the tutor took to answer, and
how sure the speech recognition was of what it heard. Sent with the session's
other measures when it ends; only numbers, never what was said.

A reply is timed from the moment the student stops speaking to the moment the
tutor starts, whatever it says first. A typed message has no such moment and
is not timed.
"""

import statistics
import time
from collections.abc import Callable

# Longer than this the tutor did not answer that turn at all (it waited for
# more, or the student went on); it is not a reply time.
LONGEST_REPLY_SECONDS = 30.0
# A final transcript under this is likely to be a mishearing.
LOW_CONFIDENCE = 0.6


class Timing:
    def __init__(self, clock: Callable[[], float] = time.monotonic) -> None:
        self._clock = clock
        self._stopped_at: float | None = None
        self._replies: list[float] = []
        self._confidences: list[float] = []

    def user_state(self, old: str, new: str) -> None:
        """The student started or stopped speaking."""
        if new == "speaking":
            self._stopped_at = None
        elif old == "speaking":
            self._stopped_at = self._clock()

    def agent_state(self, new: str) -> None:
        """The tutor started speaking: the end of a wait, if one was open."""
        if new != "speaking" or self._stopped_at is None:
            return
        waited = self._clock() - self._stopped_at
        self._stopped_at = None
        if 0 <= waited <= LONGEST_REPLY_SECONDS:
            self._replies.append(waited)

    def heard(self, confidence: float) -> None:
        """The confidence of one final transcript, from 0 to 1."""
        if 0 < confidence <= 1:
            self._confidences.append(confidence)

    def summary(self) -> dict[str, float]:
        out: dict[str, float] = {}
        if self._replies:
            replies = sorted(self._replies)
            out["replies_timed"] = len(replies)
            out["reply_ms_p50"] = round(statistics.median(replies) * 1000)
            out["reply_ms_p90"] = round(replies[max(0, int(len(replies) * 0.9 + 0.5) - 1)] * 1000)
            out["reply_ms_max"] = round(replies[-1] * 1000)
        if self._confidences:
            heard = self._confidences
            out["stt_finals"] = len(heard)
            out["stt_confidence_mean"] = round(statistics.fmean(heard), 3)
            out["stt_low_confidence_share"] = round(
                sum(c < LOW_CONFIDENCE for c in heard) / len(heard), 3
            )
        return out
