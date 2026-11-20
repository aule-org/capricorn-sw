"""Whether the voice goes up or down at the end of an utterance.

The language model only reads what speech-to-text wrote, and that has no tune:
"Is it raining?" said flat reads the same as said with the voice going up. A
pronunciation drill needs to know which, so it is measured from the audio by
rule, the way the platform checks a sum without a model: the pitch with YIN,
then how far it moved into the end of the last stretch of speech, in semitones
from the speaker's own median so a child's high voice and an adult's low one are
judged alike.

A pure function of the samples it is given: nothing here logs, stores or sends
audio.
"""

import re
from dataclasses import dataclass
from typing import Literal

import numpy as np
import numpy.typing as npt

Pattern = Literal["rising", "falling", "level"]
Samples = npt.NDArray[np.signedinteger] | npt.NDArray[np.floating]

# The pitch is tracked at this rate whatever the input's: plenty for the
# harmonics of a voice, and the frame sizes below stay fixed.
RATE = 16_000
MIN_SAMPLE_RATE = 8_000
# A man's low voice to a child's high one, with room for a child's question
# that rises past 500 Hz.
MIN_HZ = 75.0
MAX_HZ = 600.0
HOP_SECONDS = 0.01
# YIN compares this much of a frame with itself one period later: at least
# the period of the lowest voice, and short enough that a voice moving fast, as
# a child's does, stays near one pitch within it.
WINDOW_SECONDS = 0.015
# Mains hum and rumble sit under the lowest voice and would pass for a period.
RUMBLE_HZ = 50.0
# YIN's threshold: the first dip of the normalised difference under it is the
# period.
DIP = 0.1
# How deep that dip must be for a frame to be voice, from 0 (perfectly
# periodic) up: a breathy or fast-moving voice dips less deep than a steady
# one, and a hiss or a whisper hardly at all.
VOICED = 0.5
# Frames this much quieter than the loud part of the utterance are not judged
# (a breath, the room, a word trailing off), nor anything under SILENCE_RMS.
QUIET_DB = 30.0
SILENCE_RMS = 1e-3
# A voice moves less than this from one frame to the next, even a child's
# rising fast; a leap is the tracker slipping onto twice the period, or noise.
LEAP_SEMITONES = 3.0
# Less voice than this in one piece is a click or the burst of a "k".
MIN_RUN_FRAMES = 5
SMOOTH_FRAMES = 5
# Further than this from the speaker's median is a slip the rest missed: even
# a child's excited question stays within an octave.
RANGE_SEMITONES = 12.0
# With less voice than this there is nothing to judge.
MIN_VOICED_SECONDS = 0.15
# The end is judged over the last half second of voice, and not across a pause
# longer than MAX_GAP_SECONDS: the word before it is not part of the ending.
FINAL_SECONDS = 0.5
MAX_GAP_SECONDS = 0.25
# Where the voice ended up is the median of its last moments, not one frame.
END_SECONDS = 0.05
# How far the voice must move into the end to be heard as going up or down.
# Questions and statements read by synthetic voices moved 3 to 11 semitones;
# under 2 a listener hears it as level.
MOVE_SEMITONES = 2.0
POINTS = 32

_HOP = round(HOP_SECONDS * RATE)
_WINDOW = round(WINDOW_SECONDS * RATE)
_SHORTEST = int(RATE / MAX_HZ)
_LONGEST = int(np.ceil(RATE / MIN_HZ))
_FRAME = _WINDOW + _LONGEST + 1
_FFT = 1 << (_FRAME - 1).bit_length()


@dataclass(frozen=True)
class Contour:
    """The tune of one utterance, in numbers a rule can act on."""

    pattern: Pattern
    # How far the voice moved into the end of its last stretch, in semitones:
    # up from the lowest point there, or down (negative) from the highest,
    # whichever is larger.
    final_semitones: float
    voiced_seconds: float
    # The pitch from the first voiced moment to the last in POINTS steps, in
    # semitones from the speaker's median; None where there was no voice.
    points: tuple[float | None, ...]


def measure(samples: Samples, sample_rate: int) -> Contour | None:
    """The tune of one utterance of mono PCM, or None when there is too little
    voice in it to tell. Signed integer samples are full scale at their type's
    limit, float samples at 1.0. Raises ValueError for anything that is not mono audio
    at a usable rate."""
    hz = track(samples, sample_rate)
    if np.count_nonzero(~np.isnan(hz)) * HOP_SECONDS < MIN_VOICED_SECONDS:
        return None
    # Relative to the speaker's own median, so a child and an adult compare.
    semitones = 12.0 * np.log2(hz / np.nanmedian(hz))
    semitones[np.abs(semitones) > RANGE_SEMITONES] = np.nan
    voiced = np.flatnonzero(~np.isnan(semitones))
    voiced_seconds = len(voiced) * HOP_SECONDS
    if voiced_seconds < MIN_VOICED_SECONDS:
        return None

    final = _final_stretch(voiced)
    if len(final) < MIN_RUN_FRAMES:
        return None
    moved = _moved(semitones[final])
    pattern: Pattern = "level"
    if moved >= MOVE_SEMITONES:
        pattern = "rising"
    elif moved <= -MOVE_SEMITONES:
        pattern = "falling"
    return Contour(
        pattern=pattern,
        final_semitones=round(moved, 2),
        voiced_seconds=round(voiced_seconds, 2),
        points=_points(semitones[voiced[0] : voiced[-1] + 1]),
    )


def track(samples: Samples, sample_rate: int) -> npt.NDArray[np.float64]:
    """The pitch every HOP_SECONDS in Hz, NaN where there is no voice. Raises
    ValueError like ``measure``."""
    audio = _prepare(samples, sample_rate)
    if len(audio) < _FRAME:
        return np.full(0, np.nan)
    frames = np.lib.stride_tricks.sliding_window_view(audio, _FRAME)[::_HOP]

    # YIN's difference d(t) = sum (x[j] - x[j+t])^2 over the window, written as
    # energies and a cross-correlation so the FFT does the work for all frames.
    head = np.fft.rfft(frames[:, :_WINDOW], _FFT)
    whole = np.fft.rfft(frames, _FFT)
    correlation = np.fft.irfft(np.conj(head) * whole, _FFT)[:, : _LONGEST + 1]
    energy = np.concatenate([np.zeros((len(frames), 1)), np.cumsum(frames**2, axis=1)], axis=1)
    lags = np.arange(_LONGEST + 1)
    shifted = energy[:, lags + _WINDOW] - energy[:, lags]
    difference = np.maximum(energy[:, _WINDOW : _WINDOW + 1] + shifted - 2.0 * correlation, 0.0)

    # Normalised by its running mean, so the dip at the period is near 0 and
    # the curve starts at 1 instead of at 0.
    running = np.cumsum(difference[:, 1:], axis=1)
    normalised = np.ones_like(difference)
    with np.errstate(divide="ignore", invalid="ignore"):
        normalised[:, 1:] = np.where(running > 0, difference[:, 1:] * lags[1:] / running, 1.0)

    period, depth = _periods(normalised)
    rms = np.sqrt(np.mean(frames[:, :_WINDOW] ** 2, axis=1))
    loud = float(np.percentile(rms, 95))
    audible = rms >= max(SILENCE_RMS, loud * 10 ** (-QUIET_DB / 20))
    return _clean(RATE / period, audible & (depth < VOICED))


_WH = frozenset("what who whom whose where when why which how".split())
_AUXILIARIES = frozenset(
    """am is are was were do does did have has had can could will would shall should may might
    must isn't aren't wasn't weren't don't doesn't didn't haven't hasn't hadn't can't couldn't
    won't wouldn't shan't shouldn't mightn't mustn't""".split()
)


def expected_pattern(sentence: str) -> Pattern:
    """How a short English sentence ends when said the usual way, by rule.

    - A sentence that does not end with "?" falls.
    - A yes/no question, one that starts with an auxiliary or a modal ("Is it
      raining?", "Can you swim?", "Don't you like it?"), rises; unless it is a
      choice with "or" ("Is it big or small?"), which falls on its last option.
    - A wh-question ("Where do you live?", "How old are you?") falls.
    - Anything else asked with "?" rises: a statement said as a question ("You
      like apples?").

    The clause after the last comma is the one read, so a name or a greeting in
    front ("Ana, is it raining?") does not count; a comma before "or" does not
    start one. A tag question ("It's cold,
    isn't it?") is taken as a real question and rises; one that only asks for
    agreement can fall, and a rule over the text cannot tell them apart.
    """
    text = sentence.strip().replace("\u2019", "'").lower()
    if not text.endswith("?"):
        return "falling"
    clause = re.split(r",(?!\s*or\b)", text.rstrip("?"))[-1]
    words = re.findall(r"[a-z']+", clause)
    if not words:
        return "rising"
    first = words[0]
    if first.split("'")[0] in _WH:
        return "falling"
    if first in _AUXILIARIES and "or" in words:
        return "falling"
    return "rising"


def _prepare(samples: Samples, sample_rate: int) -> npt.NDArray[np.float64]:
    """The samples as floats at RATE, with no rumble under the lowest voice."""
    array = np.asarray(samples)
    if array.ndim != 1:
        raise ValueError("audio must be mono: one dimension of samples")
    if sample_rate < MIN_SAMPLE_RATE:
        raise ValueError(f"sample rate must be at least {MIN_SAMPLE_RATE} Hz")
    audio: npt.NDArray[np.float64]
    if np.issubdtype(array.dtype, np.signedinteger):
        audio = array.astype(np.float64) / 2.0 ** (8 * array.dtype.itemsize - 1)
    elif np.issubdtype(array.dtype, np.floating):
        audio = array.astype(np.float64)
    else:
        raise ValueError("samples must be signed integer or float PCM")
    if not np.all(np.isfinite(audio)):
        raise ValueError("samples must be finite")
    if len(audio) == 0:
        return audio

    # One FFT both resamples and filters: bins under the rumble are dropped,
    # and those over the new Nyquist frequency are left out.
    size = max(1, round(len(audio) * RATE / sample_rate))
    spectrum = np.fft.rfft(audio)
    spectrum[np.fft.rfftfreq(len(audio), 1.0 / sample_rate) < RUMBLE_HZ] = 0.0
    bins = size // 2 + 1
    if len(spectrum) < bins:
        spectrum = np.concatenate([spectrum, np.zeros(bins - len(spectrum), complex)])
    return np.fft.irfft(spectrum[:bins], size) * (size / len(audio))


def _periods(
    normalised: npt.NDArray[np.float64],
) -> tuple[npt.NDArray[np.float64], npt.NDArray[np.float64]]:
    """Each frame's period in samples, to a fraction of one, and how deep its
    dip is (0 for a perfectly periodic frame)."""
    rows = np.arange(len(normalised))
    search = normalised[:, _SHORTEST:]
    # The first dip under the threshold, followed down to its bottom, and not
    # the deepest one: a multiple of the period dips as deep, and taking it
    # would halve the pitch. A frame with no dip that low takes its deepest.
    threshold = np.maximum(search.min(axis=1), DIP)
    first = np.argmax(search <= threshold[:, None], axis=1)
    climbing = np.diff(search, axis=1) >= 0
    climbing &= np.arange(climbing.shape[1]) >= first[:, None]
    bottom = np.where(climbing.any(axis=1), np.argmax(climbing, axis=1), search.shape[1] - 1)
    lag = bottom + _SHORTEST

    # A parabola through the dip and its neighbours finds the period between
    # two samples, which matters for a high voice of only 27 samples a period.
    inner = np.clip(lag, 1, _LONGEST - 1)
    left, mid, right = (normalised[rows, inner + k] for k in (-1, 0, 1))
    curve = left - 2.0 * mid + right
    with np.errstate(divide="ignore", invalid="ignore"):
        shift = np.where(curve > 0, 0.5 * (left - right) / curve, 0.0)
    period = inner + np.clip(shift, -1.0, 1.0)
    return period.astype(np.float64), normalised[rows, lag]


def _clean(hz: npt.NDArray[np.float64], voiced: npt.NDArray[np.bool_]) -> npt.NDArray[np.float64]:
    """The pitch of the stretches that are voice, smoothed; NaN elsewhere."""
    # A leap ends a piece of voice and starts another.
    tone = 12.0 * np.log2(hz)
    glides = voiced[1:] & voiced[:-1] & (np.abs(np.diff(tone)) <= LEAP_SEMITONES)
    starts = np.flatnonzero(voiced & ~np.concatenate([[False], glides]))
    stops = np.flatnonzero(voiced & ~np.concatenate([glides, [False]])) + 1
    lengths = stops - starts
    # Of two pieces with a leap between them, the shorter is the slip.
    leap = stops[:-1] == starts[1:]
    shorter = np.zeros(len(lengths), dtype=bool)
    shorter[1:] |= leap & (lengths[1:] < lengths[:-1])
    shorter[:-1] |= leap & (lengths[:-1] < lengths[1:])
    keep = (lengths >= MIN_RUN_FRAMES) & ~shorter

    out = np.full_like(hz, np.nan)
    half = SMOOTH_FRAMES // 2
    for start, stop in zip(starts[keep], stops[keep], strict=True):
        # The median's window shrinks at the edges of a piece instead of
        # repeating its last frame, which may be the start of a slip.
        piece = np.pad(tone[start:stop], half, constant_values=np.nan)
        windows = np.lib.stride_tricks.sliding_window_view(piece, SMOOTH_FRAMES)
        out[start:stop] = 2.0 ** (np.nanmedian(windows, axis=1) / 12.0)
    return out


def _final_stretch(voiced: npt.NDArray[np.intp]) -> npt.NDArray[np.intp]:
    """The voiced frames of the last FINAL_SECONDS of voice, back to a pause."""
    longest_gap = round(MAX_GAP_SECONDS / HOP_SECONDS)
    earliest = voiced[-1] - round(FINAL_SECONDS / HOP_SECONDS)
    start = len(voiced) - 1
    while (
        start > 0
        and voiced[start - 1] > earliest
        and voiced[start] - voiced[start - 1] <= longest_gap
    ):
        start -= 1
    return voiced[start:]


def _moved(tune: npt.NDArray[np.float64]) -> float:
    """How far the voice moved into the end of the stretch, in semitones."""
    end = float(np.median(tune[-round(END_SECONDS / HOP_SECONDS) :]))
    up = end - float(tune.min())
    down = float(tune.max()) - end
    return up if up >= down else -down


def _points(semitones: npt.NDArray[np.float64]) -> tuple[float | None, ...]:
    """The contour in POINTS steps, each the median of the voice in it."""
    edges = np.linspace(0, len(semitones), POINTS + 1)
    points: list[float | None] = []
    for low, high in zip(edges[:-1], edges[1:], strict=True):
        start = int(low)
        values = semitones[start : max(start + 1, int(high))]
        values = values[~np.isnan(values)]
        points.append(round(float(np.median(values)), 2) if len(values) else None)
    return tuple(points)
