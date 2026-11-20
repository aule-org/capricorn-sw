from aule_agent.timing import LONGEST_REPLY_SECONDS, Timing


class Clock:
    def __init__(self):
        self.now = 0.0

    def __call__(self):
        return self.now


def test_a_reply_is_timed_from_the_end_of_speech_to_the_first_word():
    clock = Clock()
    timing = Timing(clock)

    for waited in (2.0, 3.0, 4.0, 10.0):
        timing.user_state("listening", "speaking")
        clock.now += 3
        timing.user_state("speaking", "listening")
        clock.now += waited
        timing.agent_state("thinking")
        timing.agent_state("speaking")

    summary = timing.summary()
    assert summary["replies_timed"] == 4
    assert summary["reply_ms_p50"] == 3500
    assert summary["reply_ms_p90"] == 10000
    assert summary["reply_ms_max"] == 10000


def test_what_is_not_a_reply_is_not_timed():
    clock = Clock()
    timing = Timing(clock)

    # The tutor speaks first: nobody was waiting.
    timing.agent_state("speaking")
    # The student pauses and goes on: the wait starts again from the new end.
    timing.user_state("listening", "speaking")
    timing.user_state("speaking", "listening")
    clock.now += 1
    timing.user_state("listening", "speaking")
    clock.now += 2
    timing.user_state("speaking", "listening")
    clock.now += 2.5
    timing.agent_state("speaking")
    # A turn the tutor did not answer for a long time.
    timing.user_state("listening", "speaking")
    timing.user_state("speaking", "listening")
    clock.now += LONGEST_REPLY_SECONDS + 1
    timing.agent_state("speaking")

    assert timing.summary()["replies_timed"] == 1
    assert timing.summary()["reply_ms_max"] == 2500


def test_how_sure_the_hearing_was():
    timing = Timing()
    for confidence in (0.95, 0.9, 0.4, 0.0, 1.5):
        timing.heard(confidence)

    summary = timing.summary()
    assert summary["stt_finals"] == 3
    assert summary["stt_confidence_mean"] == 0.75
    assert summary["stt_low_confidence_share"] == 0.333


def test_a_session_with_nothing_to_measure_sends_nothing():
    assert Timing().summary() == {}
