"""Build the speech and language models a session uses from the settings.

Plugins are imported inside each branch so a worker only loads the providers
it is configured for. Each plugin reads its own key from the environment;
``config`` has already checked that it is there.
"""

from livekit.agents import llm, stt, tts

from .config import Settings

# Replies are spoken, so a long one is a bug: keep the model from producing it.
# This also caps latency: the whole reply is generated before any of it is spoken
# (the output gate buffers it), so a shorter ceiling is a shorter wait to first word.
MAX_REPLY_TOKENS = 160


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
    if settings.tts == "assemblyai":
        from livekit.plugins import assemblyai

        # Every assemblyai-tts-1 voice can speak each language, but a voice sounds
        # best in its own, so pick a voice recorded in the language (an ``aai-``
        # slug, e.g. ``aai-constanza`` for Spanish) via CAPRICORN_TTS_VOICE. The
        # plugin takes an ISO code for ``language``. Without a voice the model
        # falls back to the language's default, which may be another language.
        return assemblyai.TTS(language=language, **({"voice": voice} if voice else {}))
    if settings.tts == "cartesia":
        from livekit.plugins import cartesia

        return cartesia.TTS(language=language, **({"voice": voice} if voice else {}))
    if settings.tts == "elevenlabs":
        from livekit.plugins import elevenlabs

        return elevenlabs.TTS(language=language, **({"voice_id": voice} if voice else {}))
    if settings.tts == "inworld":
        from livekit.plugins import inworld

        # Inworld wants a BCP-47 tag; the app's language is a bare code.
        bcp47 = {"es": "es-ES", "en": "en-US"}.get(language, language)
        return inworld.TTS(language=bcp47, **({"voice": voice} if voice else {}))
    from livekit.plugins import openai

    return openai.TTS(**({"voice": voice} if voice else {}))


def build_llm(settings: Settings) -> llm.LLM:
    from livekit.plugins import anthropic

    return anthropic.LLM(model=settings.llm_model, max_tokens=MAX_REPLY_TOKENS)
