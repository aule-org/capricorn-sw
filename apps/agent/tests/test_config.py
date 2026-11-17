import pytest

from capricorn_agent.config import ConfigError, Settings

COMPLETE = {
    "LIVEKIT_URL": "ws://127.0.0.1:7880",
    "LIVEKIT_API_KEY": "key",
    "LIVEKIT_API_SECRET": "secret",
    "ANTHROPIC_API_KEY": "sk-test",
    "CAPRICORN_STT": "assemblyai",
    "ASSEMBLYAI_API_KEY": "aai",
    "CAPRICORN_TTS": "cartesia",
    "CARTESIA_API_KEY": "car",
}


def test_reads_a_complete_environment_with_defaults():
    settings = Settings.from_env(COMPLETE)

    assert settings.agent_name == "capricorn-tutor"
    assert (settings.stt, settings.tts) == ("assemblyai", "cartesia")
    assert settings.language == "es"
    assert settings.session_max_minutes == 15
    assert settings.tts_voice is None


def test_reports_every_problem_at_once():
    with pytest.raises(ConfigError) as error:
        Settings.from_env({"CAPRICORN_STT": "deepgram", "CAPRICORN_SESSION_MAX_MINUTES": "0"})

    message = str(error.value)
    for expected in (
        "CAPRICORN_TTS must be one of",
        "LIVEKIT_URL is not set",
        "ANTHROPIC_API_KEY is not set",
        "DEEPGRAM_API_KEY is not set",
        "CAPRICORN_SESSION_MAX_MINUTES",
    ):
        assert expected in message


def test_requires_the_key_of_the_chosen_provider_only():
    env = {**COMPLETE, "CAPRICORN_TTS": "openai"}

    with pytest.raises(ConfigError, match="^OPENAI_API_KEY is not set$"):
        Settings.from_env(env)


def test_a_blank_key_counts_as_missing():
    with pytest.raises(ConfigError, match="CARTESIA_API_KEY is not set"):
        Settings.from_env({**COMPLETE, "CARTESIA_API_KEY": "  "})


def test_rejects_an_unsupported_language():
    with pytest.raises(ConfigError, match="CAPRICORN_LANGUAGE"):
        Settings.from_env({**COMPLETE, "CAPRICORN_LANGUAGE": "fr"})
