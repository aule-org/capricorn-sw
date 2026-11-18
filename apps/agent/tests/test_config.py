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


def test_each_gate_is_configured_by_its_key_and_no_key_is_shown():
    assert not Settings.from_env(COMPLETE).gates_complete
    assert not Settings.from_env({**COMPLETE, "OPENAI_API_KEY": "sk-secret"}).gates_complete
    assert not Settings.from_env({**COMPLETE, "TYPESAFE_API_KEY": "ts-secret"}).gates_complete

    settings = Settings.from_env(
        {**COMPLETE, "OPENAI_API_KEY": " sk-secret ", "TYPESAFE_API_KEY": "ts-secret"}
    )

    assert settings.gates_complete
    assert (settings.openai_api_key, settings.typesafe_api_key) == ("sk-secret", "ts-secret")
    assert "secret" not in repr(settings)


def test_assemblyai_speaks_with_the_key_it_also_hears_with():
    # The STT key doubles as the TTS key, so choosing assemblyai for speech
    # needs nothing beyond what hearing already required.
    settings = Settings.from_env({**COMPLETE, "CAPRICORN_TTS": "assemblyai"})

    assert settings.tts == "assemblyai"


def test_the_moderation_key_is_also_the_speech_key_when_openai_speaks():
    settings = Settings.from_env({**COMPLETE, "CAPRICORN_TTS": "openai", "OPENAI_API_KEY": "sk"})

    assert settings.tts == "openai"
    assert settings.openai_api_key == "sk"


def test_a_blank_gate_key_means_no_gate_and_a_broken_one_is_a_problem():
    blank = Settings.from_env({**COMPLETE, "OPENAI_API_KEY": " ", "TYPESAFE_API_KEY": ""})
    assert (blank.openai_api_key, blank.typesafe_api_key) == (None, None)

    with pytest.raises(ConfigError) as error:
        Settings.from_env(
            {**COMPLETE, "OPENAI_API_KEY": "sk test", "TYPESAFE_API_KEY": "ts\x00test"}
        )
    assert "OPENAI_API_KEY must not contain" in str(error.value)
    assert "TYPESAFE_API_KEY must not contain" in str(error.value)
    assert "sk test" not in str(error.value)
