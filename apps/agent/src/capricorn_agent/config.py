"""Settings for the worker, read from the environment once at start.

A worker with a missing key would accept a session and then fail in front of
the user, so ``Settings.from_env`` reports every problem at once and the worker
refuses to start.
"""

import os
from collections.abc import Mapping
from dataclasses import dataclass

# The environment variable each provider's LiveKit plugin reads its key from.
STT_KEYS = {
    "assemblyai": "ASSEMBLYAI_API_KEY",
    "deepgram": "DEEPGRAM_API_KEY",
    "openai": "OPENAI_API_KEY",
}
TTS_KEYS = {
    "cartesia": "CARTESIA_API_KEY",
    "elevenlabs": "ELEVEN_API_KEY",
    "openai": "OPENAI_API_KEY",
}
LLM_KEY = "ANTHROPIC_API_KEY"
LIVEKIT_KEYS = ("LIVEKIT_URL", "LIVEKIT_API_KEY", "LIVEKIT_API_SECRET")

LANGUAGES = ("es", "en")


class ConfigError(ValueError):
    """The environment cannot run a worker. The message lists every problem."""


@dataclass(frozen=True)
class Settings:
    agent_name: str
    stt: str
    tts: str
    tts_voice: str | None
    llm_model: str
    language: str
    session_max_minutes: int

    @classmethod
    def from_env(cls, env: Mapping[str, str] | None = None) -> "Settings":
        env = os.environ if env is None else env

        def get(name: str, default: str = "") -> str:
            return env.get(name, "").strip() or default

        problems: list[str] = []

        stt = get("CAPRICORN_STT").lower()
        if stt not in STT_KEYS:
            problems.append(f"CAPRICORN_STT must be one of: {', '.join(STT_KEYS)}")
        tts = get("CAPRICORN_TTS").lower()
        if tts not in TTS_KEYS:
            problems.append(f"CAPRICORN_TTS must be one of: {', '.join(TTS_KEYS)}")

        required = [*LIVEKIT_KEYS, LLM_KEY]
        if stt in STT_KEYS:
            required.append(STT_KEYS[stt])
        if tts in TTS_KEYS:
            required.append(TTS_KEYS[tts])
        for name in dict.fromkeys(required):
            if not get(name):
                problems.append(f"{name} is not set")

        language = get("CAPRICORN_LANGUAGE", "es").lower()
        if language not in LANGUAGES:
            problems.append(f"CAPRICORN_LANGUAGE must be one of: {', '.join(LANGUAGES)}")

        raw_minutes = get("CAPRICORN_SESSION_MAX_MINUTES", "15")
        minutes = int(raw_minutes) if raw_minutes.isdecimal() else 0
        if not 1 <= minutes <= 120:
            problems.append("CAPRICORN_SESSION_MAX_MINUTES must be a whole number from 1 to 120")

        if problems:
            raise ConfigError("; ".join(problems))

        return cls(
            agent_name=get("CAPRICORN_AGENT_NAME", "capricorn-tutor"),
            stt=stt,
            tts=tts,
            tts_voice=get("CAPRICORN_TTS_VOICE") or None,
            llm_model=get("CAPRICORN_LLM_MODEL", "claude-sonnet-5-5"),
            language=language,
            session_max_minutes=minutes,
        )
