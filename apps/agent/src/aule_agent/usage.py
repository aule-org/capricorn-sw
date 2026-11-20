"""What a session cost in model tokens: one line in the log when it ends.

Only counts are kept, never text. ``cached`` is the part of the input that was
read from the prompt cache, which is billed at a fraction of the rest.
"""

from dataclasses import dataclass


@dataclass
class Usage:
    calls: int = 0
    input_tokens: int = 0
    cached_tokens: int = 0
    output_tokens: int = 0

    def add(self, metrics: object) -> None:
        """Counts one model call from the metrics the model plugin emits."""
        self.calls += 1
        self.input_tokens += _count(metrics, "prompt_tokens")
        self.cached_tokens += _count(metrics, "prompt_cached_tokens")
        self.output_tokens += _count(metrics, "completion_tokens")

    def summary(self) -> dict[str, int]:
        return {
            "llm_calls": self.calls,
            "input_tokens": self.input_tokens,
            "cached_input_tokens": self.cached_tokens,
            "output_tokens": self.output_tokens,
        }


def _count(metrics: object, name: str) -> int:
    value = getattr(metrics, name, 0)
    return value if isinstance(value, int) and value > 0 else 0
