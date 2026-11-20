import numpy as np
import pytest

from aule_agent.prosody import POINTS, Contour, expected_pattern, measure, track


def voice(tune, rate=16_000, loudness=0.3, harmonics=10):
    """A voice-like tone: harmonics falling off like a vowel's, its pitch moving
    in straight lines (on a log scale) between the (seconds, Hz) points of
    ``tune``, with short fades so it starts and stops like a syllable."""
    times, pitches = zip(*tune, strict=True)
    t = np.arange(int(times[-1] * rate)) / rate
    hz = np.exp(np.interp(t, times, np.log(pitches)))
    phase = 2 * np.pi * np.cumsum(hz) / rate
    tone = sum(np.sin(k * phase) / k for k in range(1, harmonics + 1))
    fade = np.minimum(1.0, np.minimum(t, t[-1] - t) / 0.02)
    return loudness * fade * tone / np.max(np.abs(tone))


def silence(seconds, rate=16_000):
    return np.zeros(int(seconds * rate))


def hiss(seconds, rate=16_000, loudness=0.05, seed=1):
    return np.random.default_rng(seed).normal(0.0, loudness, int(seconds * rate))


def pcm(audio):
    return np.round(audio * 32767).astype(np.int16)


def semitones(low, high):
    return 12 * np.log2(high / low)


# An utterance that glides from one pitch to another over 0.4 s near its end,
# like a question going up or a statement coming down, and holds the last one.
def ending(start, end, rate=16_000):
    return voice([(0, start), (0.7, start), (1.1, end), (1.2, end)], rate)


def test_a_voice_that_goes_up_at_the_end_rises():
    contour = measure(pcm(ending(200, 300)), 16_000)

    assert contour is not None
    assert contour.pattern == "rising"
    assert contour.final_semitones == pytest.approx(semitones(200, 300), abs=0.3)
    assert contour.voiced_seconds == pytest.approx(1.2, abs=0.05)


def test_a_voice_that_comes_down_at_the_end_falls():
    contour = measure(pcm(ending(300, 200)), 16_000)

    assert contour is not None
    assert contour.pattern == "falling"
    assert contour.final_semitones == pytest.approx(-semitones(200, 300), abs=0.3)


def test_a_voice_that_stays_on_one_note_is_level():
    contour = measure(pcm(voice([(0, 220), (1.2, 220)])), 16_000)

    assert contour is not None
    assert contour.pattern == "level"
    assert contour.final_semitones == pytest.approx(0, abs=0.1)
    assert all(p is not None and abs(p) < 0.1 for p in contour.points)


def test_a_small_wobble_at_the_end_is_still_level():
    # Half a tone up is not a question: a rule must not hear one.
    contour = measure(pcm(ending(220, 220 * 2 ** (0.5 / 12))), 16_000)

    assert contour is not None and contour.pattern == "level"


def test_a_childs_high_voice_rising_is_followed_to_the_top():
    audio = ending(300, 450)

    pitch = track(audio, 16_000)
    contour = measure(pcm(audio), 16_000)

    voiced = pitch[~np.isnan(pitch)]
    assert voiced[5] == pytest.approx(300, rel=0.01)
    assert voiced[-3] == pytest.approx(450, rel=0.02)
    assert contour is not None and contour.pattern == "rising"
    assert contour.final_semitones == pytest.approx(semitones(300, 450), abs=0.3)


def test_a_mans_low_voice_falling_is_followed_to_the_bottom():
    audio = ending(110, 90)

    pitch = track(audio, 16_000)
    contour = measure(pcm(audio), 16_000)

    voiced = pitch[~np.isnan(pitch)]
    assert voiced[5] == pytest.approx(110, rel=0.01)
    assert voiced[-3] == pytest.approx(90, rel=0.02)
    assert contour is not None and contour.pattern == "falling"
    assert contour.final_semitones == pytest.approx(-semitones(90, 110), abs=0.3)


@pytest.mark.parametrize("hz", [75, 110, 220, 350, 500])
def test_the_pitch_of_a_steady_voice_is_found_across_the_range(hz):
    pitch = track(voice([(0, hz), (0.5, hz)]), 16_000)

    voiced = pitch[~np.isnan(pitch)]
    assert len(voiced) >= 0.4 / 0.01
    assert np.median(voiced) == pytest.approx(hz, rel=0.005)


@pytest.mark.parametrize("rate", [16_000, 24_000, 48_000])
def test_the_sample_rate_does_not_change_the_answer(rate):
    contour = measure(pcm(ending(220, 330, rate)), rate)

    assert contour is not None
    assert contour.pattern == "rising"
    assert contour.final_semitones == pytest.approx(semitones(220, 330), abs=0.3)


def test_float_and_integer_samples_give_the_same_tune():
    audio = ending(250, 180)

    assert measure(audio, 16_000) == measure(pcm(audio), 16_000)
    assert measure(audio.astype(np.float32), 16_000) == measure(pcm(audio), 16_000)


def test_an_unvoiced_sound_after_the_voice_does_not_hide_the_rise():
    # "...cats?": the voice goes up, then a hiss with no pitch.
    audio = np.concatenate([ending(200, 300), hiss(0.15), silence(0.2)])

    contour = measure(pcm(audio), 16_000)

    assert contour is not None and contour.pattern == "rising"
    assert contour.voiced_seconds == pytest.approx(1.2, abs=0.05)
    assert contour.points[-1] is not None and contour.points[-1] > 3


def test_a_voice_that_creaks_an_octave_down_after_rising_still_rises():
    # A voice trailing off in creak, or the tracker slipping onto twice the
    # period, drops the pitch an octave at once: no voice moves that fast.
    audio = voice([(0, 200), (0.7, 200), (1.1, 300), (1.2, 300), (1.201, 150), (1.27, 150)])

    contour = measure(pcm(audio), 16_000)

    assert contour is not None and contour.pattern == "rising"
    assert contour.final_semitones == pytest.approx(semitones(200, 300), abs=0.5)


def test_the_end_is_judged_on_the_last_word_and_not_across_a_pause():
    # A high word, a pause, then a last word on one lower note: the ending is
    # level, not a fall from the word before the pause.
    audio = np.concatenate(
        [voice([(0, 300), (0.4, 300)]), silence(0.4), voice([(0, 200), (0.35, 200)])]
    )

    contour = measure(pcm(audio), 16_000)

    assert contour is not None and contour.pattern == "level"
    first, last = contour.points[0], contour.points[-1]
    assert first is not None and last is not None
    assert first - last == pytest.approx(semitones(200, 300), abs=0.2)
    assert None in contour.points


def test_a_peak_on_the_last_word_that_comes_back_down_falls():
    # The voice climbs onto the stressed syllable and drops off it, as a
    # statement does: the end is a fall from the peak, not a rise from the start.
    contour = measure(
        pcm(voice([(0, 200), (0.6, 200), (0.8, 260), (1.05, 190), (1.15, 190)])), 16_000
    )

    assert contour is not None and contour.pattern == "falling"
    assert contour.final_semitones == pytest.approx(-semitones(190, 260), abs=0.5)


def test_a_dip_on_the_last_word_that_comes_back_up_rises():
    contour = measure(
        pcm(voice([(0, 220), (0.6, 220), (0.8, 180), (1.05, 260), (1.15, 260)])), 16_000
    )

    assert contour is not None and contour.pattern == "rising"
    assert contour.final_semitones == pytest.approx(semitones(180, 260), abs=0.5)


def test_a_small_lift_after_a_big_fall_is_still_a_fall():
    # Synthetic and real voices often lift a little as the voice stops; the
    # larger movement into the end is the one heard.
    contour = measure(
        pcm(voice([(0, 250), (0.6, 250), (1.0, 160), (1.1, 175), (1.15, 175)])), 16_000
    )

    assert contour is not None and contour.pattern == "falling"


def test_noise_in_the_room_does_not_hide_the_tune():
    audio = ending(200, 290) + hiss(1.2, loudness=0.02, seed=7)

    contour = measure(pcm(audio), 16_000)

    assert contour is not None and contour.pattern == "rising"


def test_a_quiet_voice_is_measured_like_a_loud_one():
    loud = measure(pcm(ending(200, 300)), 16_000)
    quiet = measure(pcm(ending(200, 300) * 0.03), 16_000)

    assert quiet is not None and loud is not None
    assert quiet.pattern == loud.pattern
    assert quiet.final_semitones == pytest.approx(loud.final_semitones, abs=0.1)


@pytest.mark.parametrize(
    "audio",
    [
        silence(1.0),
        hiss(1.0),
        np.zeros(0),
        # Too short to say anything: one tenth of a second of voice.
        voice([(0, 200), (0.1, 200)]),
    ],
    ids=["silence", "noise", "empty", "too short"],
)
def test_nothing_is_said_about_audio_without_enough_voice(audio):
    assert measure(pcm(audio), 16_000) is None


def test_the_points_follow_the_contour_relative_to_the_speakers_median():
    contour = measure(pcm(voice([(0, 200), (1.0, 400)])), 16_000)

    assert isinstance(contour, Contour)
    assert len(contour.points) == POINTS
    values = [p for p in contour.points if p is not None]
    assert len(values) == POINTS
    # A steady glide over one octave: from six semitones under the median to
    # six over it, always going up.
    assert values[0] == pytest.approx(-6, abs=0.6)
    assert values[-1] == pytest.approx(6, abs=0.6)
    assert all(b >= a for a, b in zip(values, values[1:], strict=False))


def test_a_short_utterance_still_has_all_its_points():
    contour = measure(pcm(voice([(0, 200), (0.2, 200)])), 16_000)

    assert contour is not None and len(contour.points) == POINTS


@pytest.mark.parametrize(
    ("samples", "rate"),
    [
        (np.zeros((2, 16_000), dtype=np.int16), 16_000),
        (np.zeros(16_000, dtype=np.int16), 4_000),
        (np.array(["a", "b"]), 16_000),
        (np.zeros(16_000, dtype=np.uint8), 16_000),
        (np.array([0.0, np.nan] * 8_000), 16_000),
    ],
    ids=["stereo", "rate too low", "not audio", "unsigned", "not finite"],
)
def test_what_is_not_mono_audio_is_refused(samples, rate):
    with pytest.raises(ValueError):
        measure(samples, rate)


@pytest.mark.parametrize(
    ("sentence", "expected"),
    [
        ("Is it raining?", "rising"),
        ("Do you like apples?", "rising"),
        ("Can you swim?", "rising"),
        ("Does she have a dog?", "rising"),
        ("Have you finished your homework?", "rising"),
        ("Should we go?", "rising"),
        ("Isn't it cold?", "rising"),
        ("Don\u2019t you like it?", "rising"),
        ("Ana, is it raining?", "rising"),
        ("You like apples?", "rising"),
        ("It's cold, isn't it?", "rising"),
        ("What is your name?", "falling"),
        ("What's your name?", "falling"),
        ("Where do you live?", "falling"),
        ("How old are you?", "falling"),
        ("Which book do you want?", "falling"),
        ("Hello, who is your teacher?", "falling"),
        ("Is it big or small?", "falling"),
        ("Do you want tea, or milk?", "falling"),
        ("It is raining.", "falling"),
        ("I like apples", "falling"),
        ("Close the door!", "falling"),
        ("Is it raining.", "falling"),
        ("  is it raining?  ", "rising"),
    ],
)
def test_the_expected_ending_follows_the_kind_of_sentence(sentence, expected):
    assert expected_pattern(sentence) == expected
