import numpy as np

from aule_agent import visuals
from aule_agent.drill import KEEP_SECONDS, MAX_TRIES, Drill, visual

RATE = 48_000


def voice(start_hz: float, end_hz: float, seconds: float = 1.2) -> np.ndarray:
    """A voice-like tone gliding from one pitch to another, as int16 PCM."""
    hz = np.linspace(start_hz, end_hz, int(seconds * RATE))
    phase = 2 * np.pi * np.cumsum(hz) / RATE
    tone = sum(np.sin(k * phase) / k for k in range(1, 6))
    silence = np.zeros(int(0.3 * RATE))
    return (np.concatenate([silence, 0.3 * tone / np.max(np.abs(tone)), silence]) * 32767).astype(
        np.int16
    )


def said(drill: Drill, samples: np.ndarray) -> None:
    """Hands the audio over in 10 ms frames, as the session does, then ends the turn."""
    for start in range(0, len(samples), RATE // 100):
        drill.hear(samples[start : start + RATE // 100], RATE)
    drill.took()


def test_a_question_that_rises_is_right_and_closes_the_drill():
    drill = Drill()
    assert "check_phrase" in drill.pose("Is it raining?")
    said(drill, voice(220, 330))
    text, contour = drill.check()
    assert contour is not None and contour.pattern == "rising"
    assert text.startswith("Their voice went up at the end, as a yes/no question does")
    assert not drill.open


def test_a_question_that_falls_is_tried_again_and_then_left_for_later():
    drill = Drill()
    drill.pose("Do you like pizza?")
    for attempt in range(1, MAX_TRIES + 1):
        said(drill, voice(300, 200))
        text, contour = drill.check()
        assert contour is not None and contour.pattern == "falling"
        assert "went down at the end; a yes/no question ends going up" in text
        if attempt < MAX_TRIES:
            assert "try again" in text and drill.open
    assert "last try for now" in text and not drill.open


def test_a_statement_should_fall_and_a_wh_question_too():
    for sentence in ["I have a red ball.", "Where do you live?"]:
        drill = Drill()
        drill.pose(sentence)
        said(drill, voice(260, 180))
        text, _ = drill.check()
        assert "Say so warmly" in text, sentence


def test_without_a_spoken_try_or_with_too_little_voice_nothing_is_judged():
    drill = Drill()
    drill.pose("Can you swim?")
    assert drill.check()[0].startswith("No spoken try was heard")
    said(drill, np.zeros(RATE, dtype=np.int16))
    text, contour = drill.check()
    assert contour is None and text.startswith("Too little voice")
    assert drill.open and drill.tries == 0


def test_audio_is_kept_only_while_a_drill_is_open_and_only_its_last_seconds():
    drill = Drill()
    drill.hear(voice(220, 330), RATE)
    drill.took()
    assert drill.check()[0].startswith("No sentence is registered")
    drill.pose("Is it big?")
    for _ in range(20):
        drill.hear(np.zeros(RATE, dtype=np.int16), RATE)
    assert drill._held <= (KEEP_SECONDS + 1) * RATE
    drill.close()
    assert drill._held == 0 and not drill._frames


def test_the_tune_is_drawn_within_the_contract_and_the_model_cannot_draw_one():
    drill = Drill()
    drill.pose("Is it raining?")
    said(drill, voice(220, 330))
    _, contour = drill.check()
    assert contour is not None
    drawn = visual(contour, "rising", "es")
    assert visuals.problems(drawn) == [] and drawn["title"] == "Tu voz"
    tool = visuals.tool_schema()["parameters"]
    assert "contour" not in tool["properties"]["kind"]["enum"]
    assert "segments" not in tool["properties"]
