"""A second opinion on a reply the moderation provider is unsure about.

The provider scores ordinary teaching sentences ("count the filled marbles one
by one with your finger") close to sentences no child should hear, so a score
alone cannot tell them apart at the strictness a child needs. When the only
thing found in a reply of the tutor are scores the provider itself did not act
on, a small model reads that one sentence and answers a single word.
Anything but a clear "safe" in time keeps the reply stopped.
"""

import asyncio
import logging
import time
from typing import Any

logger = logging.getLogger("aule.agent.opinion")

MODEL = "claude-haiku-5-5"
TIMEOUT_SECONDS = 2.0
# Room for the one word, and for a model that thinks before it answers.
MAX_TOKENS = 400

QUESTION = """You review one reply that a tutor is about to say out loud to a child \
between 6 and 16 during a school tutoring session. Answer with exactly one word: SAFE or UNSAFE.

UNSAFE: anything sexual or romantic, the body spoken of in a sexual way, flirting, asking \
for photos or for secrecy; an insult, a put-down, mockery or a threat; violence; contempt \
for a group of people; or anything else a careful parent would not want said to their child.
SAFE: ordinary teaching, such as counting or touching objects with a finger, shaded, filled \
or empty shapes, objects named by how they look, working together, going slowly, encouragement.

If you are not sure, answer UNSAFE."""


class SecondOpinion:
    """Asks a small model whether one reply is fit for a child."""

    def __init__(self, *, client: Any = None) -> None:
        if client is None:
            from anthropic import AsyncAnthropic

            # The key is the one the tutor's own model uses, read from the environment.
            client = AsyncAnthropic(timeout=TIMEOUT_SECONDS, max_retries=0)
        self._client = client

    async def safe(self, text: str) -> bool:
        """True only for a clear "safe" in time. Never raises."""
        started = time.monotonic()
        verdict = False
        try:
            async with asyncio.timeout(TIMEOUT_SECONDS):
                answer = await self._client.messages.create(
                    model=MODEL,
                    max_tokens=MAX_TOKENS,
                    system=QUESTION,
                    messages=[{"role": "user", "content": text}],
                )
            words = "".join(
                block.text for block in answer.content if getattr(block, "type", "") == "text"
            )
            verdict = words.strip().upper() == "SAFE"
        except asyncio.CancelledError:
            raise
        except Exception as error:  # no opinion is not a yes
            logger.warning("second opinion unavailable", extra={"error": type(error).__name__})
        logger.info(
            "second opinion",
            extra={"safe": verdict, "duration_ms": round((time.monotonic() - started) * 1000)},
        )
        return verdict

    async def close(self) -> None:
        await self._client.close()
