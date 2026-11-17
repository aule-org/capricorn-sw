"""Build the speech and language models a session uses from the settings.

Plugins are imported inside each branch so a worker only loads the providers
it is configured for. Each plugin reads its own key from the environment;
``config`` has already checked that it is there.
"""

from livekit.agents import llm, stt, tts

from .config import Settings

# Replies are spoken, so a long one is a bug: keep the model from producing it.
MAX_REPLY_TOKENS = 400


def build_stt(settings: Settings, language: str) -> stt.STT:
    if settings.stt == "assemblyai":
        from livekit.plugins import assemblyai

        return assemblyai.STT(language_code=language)
    if settings.stt == "deepgram":
        from livekit.plugins import deepgram

        return deepgram.STT(language=language)
    from livekit.plugins import openai

    return openai.STT(language=language)


def build_tts(settings: Settings, language: str) -> tts.TTS:
    voice = settings.tts_voice
    if settings.tts == "cartesia":
        from livekit.plugins import cartesia

        return cartesia.TTS(language=language, **({"voice": voice} if voice else {}))
    if settings.tts == "elevenlabs":
        from livekit.plugins import elevenlabs

        return elevenlabs.TTS(language=language, **({"voice_id": voice} if voice else {}))
    from livekit.plugins import openai

    return openai.TTS(**({"voice": voice} if voice else {}))


def build_llm(settings: Settings) -> llm.LLM:
    from livekit.plugins import anthropic

    return anthropic.LLM(model=settings.llm_model, max_tokens=MAX_REPLY_TOKENS)
