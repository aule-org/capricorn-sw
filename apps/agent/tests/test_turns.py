from livekit.agents import AgentSession

from aule_agent.worker import SPEECH_LIKELIHOOD, SPEECH_MIN_SECONDS, TURNS


async def test_the_session_takes_the_turn_options_for_noisy_homes():
    options = AgentSession(turn_handling=TURNS).options.turn_handling
    # A noise with no words in it does not stop the tutor, and a short one is not speech.
    assert options["interruption"]["min_words"] >= 1
    assert options["interruption"]["min_duration"] >= 0.8
    assert options["interruption"]["mode"] == "vad"
    # A noise that stopped it all the same is undone: LiveKit's default, kept.
    assert options["interruption"]["resume_false_interruption"] is True
    # A child's pause in the middle of a sentence does not end the turn at once.
    assert options["endpointing"]["min_delay"] >= 0.7
    assert 0.5 < SPEECH_LIKELIHOOD < 0.7 and 0.05 < SPEECH_MIN_SECONDS <= 0.2


async def test_push_to_talk_keeps_the_same_options_with_manual_turns():
    options = AgentSession(
        turn_handling={**TURNS, "turn_detection": "manual"}
    ).options.turn_handling
    assert options["turn_detection"] == "manual"
    assert options["interruption"]["min_words"] == TURNS["interruption"]["min_words"]
