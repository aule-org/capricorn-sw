"""What a session used of each paid service: model tokens, seconds of audio
heard, characters spoken. One line in the log when it ends, and the platform's
cost estimate is made from it.

Only counts are kept, never text. ``cached`` is the part of the input that was
read from the prompt cache, which is billed at a fraction of the rest;
``cache_write`` is what was written to it, billed a little above the rest.
"""

from dataclasses import dataclass


@dataclass
class Usage:
    calls: int = 0
    input_tokens: int = 0
    cached_tokens: int = 0
    output_tokens: int = 0
    cache_write_tokens: int = 0
    # Speech to text bills the audio streamed to it; text to speech, the characters.
    stt_seconds: float = 0.0
    tts_chars: int = 0

    def add(self, metrics: object) -> None:
        """Counts one model call from the metrics the model plugin emits."""
        self.calls += 1
        self.input_tokens += _count(metrics, "prompt_tokens")
        self.cached_tokens += _count(metrics, "prompt_cached_tokens")
        self.cache_write_tokens += _count(metrics, "cache_creation_tokens")
        self.output_tokens += _count(metrics, "completion_tokens")

    def heard(self, metrics: object) -> None:
        """Counts the audio the speech-to-text plugin was sent."""
        seconds = getattr(metrics, "audio_duration", 0.0)
        if isinstance(seconds, (int, float)) and not isinstance(seconds, bool) and seconds > 0:
            self.stt_seconds += float(seconds)

    def spoke(self, metrics: object) -> None:
        """Counts the characters the text-to-speech plugin was given."""
        self.tts_chars += _count(metrics, "characters_count")

    def summary(self) -> dict[str, int | float]:
        return {
            "llm_calls": self.calls,
            "input_tokens": self.input_tokens,
            "cached_input_tokens": self.cached_tokens,
            "cache_write_tokens": self.cache_write_tokens,
            "output_tokens": self.output_tokens,
            "stt_seconds": round(self.stt_seconds, 1),
            "tts_chars": self.tts_chars,
        }


def _count(metrics: object, name: str) -> int:
    value = getattr(metrics, name, 0)
    return value if isinstance(value, int) and value > 0 else 0
