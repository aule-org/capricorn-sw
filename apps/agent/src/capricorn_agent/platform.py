"""The agent's side of the platform: where the tutor gets what it should know
about the student, and where it reports what was said and what a safety gate
stopped.

Nothing here is allowed to slow the conversation down. The plan is fetched
once, before the greeting, with a short deadline. After that every call runs
in the background: a turn is reported after it happened, and guidance from the
supervisor is applied to the tutor's next turns whenever it arrives. If the
platform is unreachable the tutor keeps working without it.
"""

import asyncio
import json
import logging
import os
import time
from collections.abc import Awaitable, Callable, Mapping
from dataclasses import dataclass
from typing import Any

import aiohttp

from .safety import FLAG_ACTIONS, FLAG_CATEGORIES, FLAG_GATES
from .tutor import Persona, persona_from

logger = logging.getLogger("capricorn.agent.platform")

START_TIMEOUT_SECONDS = 6.0
REPORT_TIMEOUT_SECONDS = 10.0
# Turns kept while the platform is unreachable; older ones are dropped.
MAX_PENDING_TURNS = 50
ROLES = {"user": "child", "assistant": "tutor"}
# One flag per gate, action and gravest category in this time, so a topic a
# child keeps returning to does not flood the reports.
FLAG_COOLDOWN_SECONDS = 300.0
FLAG_TIMEOUT_SECONDS = 2.0
MAX_FLAGS_IN_FLIGHT = 4

Send = Callable[[str, str, dict[str, Any] | None, float], Awaitable[dict[str, Any] | None]]


@dataclass(frozen=True)
class PlatformSession:
    url: str
    token: str


def platform_session(
    dispatch_metadata: str, env: Mapping[str, str] | None = None
) -> PlatformSession | None:
    """The platform session this job belongs to, if it was created by one."""
    env = os.environ if env is None else env
    url = env.get("CAPRICORN_PLATFORM_URL", "").strip().rstrip("/")
    try:
        metadata = json.loads(dispatch_metadata or "{}")
    except ValueError:
        return None
    token = metadata.get("platform_token") if isinstance(metadata, dict) else None
    if not url or not isinstance(token, str) or not token:
        return None
    return PlatformSession(url=url, token=token)


class Platform:
    def __init__(
        self,
        session: PlatformSession,
        send: Send | None = None,
        clock: Callable[[], float] = time.monotonic,
    ) -> None:
        self._session = session
        self._send = send or self._http
        # Filled by ``start`` from the agent-session body; the tutor's persona.
        self.persona: Persona | None = None
        self._clock = clock
        self._flagged_at: dict[tuple[str, str, str], float] = {}
        self._flags: set[asyncio.Task[None]] = set()
        self._http_session: aiohttp.ClientSession | None = None
        self._pending: list[dict[str, Any]] = []
        self._next_seq = 0
        self._after_guidance = 0
        self._wake = asyncio.Event()

    async def _http(
        self, method: str, path: str, body: dict[str, Any] | None, timeout: float
    ) -> dict[str, Any] | None:
        if self._http_session is None:
            self._http_session = aiohttp.ClientSession()
        async with self._http_session.request(
            method,
            f"{self._session.url}{path}",
            json=body,
            headers={"Authorization": f"Bearer {self._session.token}"},
            timeout=aiohttp.ClientTimeout(total=timeout),
            allow_redirects=False,
        ) as response:
            if response.status == 204:
                return {}
            if response.status != 200:
                # The body may repeat what was sent, so only the status is kept.
                raise RuntimeError(f"platform answered {response.status}")
            return await response.json()

    async def _call(
        self, method: str, path: str, body: dict[str, Any] | None, timeout: float
    ) -> dict[str, Any] | None:
        try:
            return await self._send(method, path, body, timeout)
        except asyncio.CancelledError:
            raise
        except Exception as error:  # the tutor never stops because of the platform
            logger.warning(
                "platform call failed", extra={"path": path, "error": type(error).__name__}
            )
            return None

    async def start(self) -> str:
        """What the tutor should know before it says hello. Empty if unknown."""
        answer = (await self._call("GET", "/v1/agent/session", None, START_TIMEOUT_SECONDS)) or {}
        tutor = answer.get("tutor")
        if isinstance(tutor, dict):
            self.persona = persona_from(tutor.get("name"), tutor.get("persona"), tutor.get("note"))
        instructions = answer.get("instructions")
        return instructions if isinstance(instructions, str) else ""

    def heard(self, role: str, text: str | None) -> None:
        """Queues one finished turn. Never waits."""
        text = (text or "").strip()
        if role not in ROLES or not text:
            return
        self._pending.append({"seq": self._next_seq, "role": ROLES[role], "content": text})
        self._next_seq += 1
        del self._pending[:-MAX_PENDING_TURNS]
        self._wake.set()

    def flag(self, gate: str, action: str, categories: list[str]) -> None:
        """Queues the report of a turn a safety gate stopped. Never waits, never raises.

        Only the fixed vocabulary of ``safety`` is sent; anything else is a bug
        in the caller and is dropped rather than forwarded.
        """
        categories = list(dict.fromkeys(categories))
        if (
            gate not in FLAG_GATES
            or action not in FLAG_ACTIONS
            or not categories
            or any(category not in FLAG_CATEGORIES for category in categories)
        ):
            logger.error("flag dropped: outside the fixed vocabulary")
            return
        key = (gate, action, categories[0])
        last = self._flagged_at.get(key)
        cooling = last is not None and self._clock() - last < FLAG_COOLDOWN_SECONDS
        if cooling or len(self._flags) >= MAX_FLAGS_IN_FLIGHT:
            return
        self._flagged_at[key] = self._clock()
        task = asyncio.create_task(self._send_flag(key, categories))
        self._flags.add(task)
        task.add_done_callback(self._flags.discard)

    async def _send_flag(self, key: tuple[str, str, str], categories: list[str]) -> None:
        answer = await self._call(
            "POST",
            "/v1/agent/session/flags",
            {"gate": key[0], "action": key[1], "categories": categories},
            FLAG_TIMEOUT_SECONDS,
        )
        if answer is None:
            # A flag that did not arrive does not use up the cooldown.
            self._flagged_at.pop(key, None)

    async def report_once(self) -> str | None:
        """Sends the queued turns and returns the newest guidance, if any."""
        turns, self._pending = self._pending, []
        answer = await self._call(
            "POST",
            "/v1/agent/session/turns",
            {"turns": turns, "after_guidance": self._after_guidance},
            REPORT_TIMEOUT_SECONDS,
        )
        if answer is None:
            # Kept for the next report; the platform stores a repeated turn once.
            self._pending = (turns + self._pending)[-MAX_PENDING_TURNS:]
            return None
        newest = None
        for item in answer.get("guidance") or []:
            if isinstance(item.get("id"), int) and isinstance(item.get("instructions"), str):
                self._after_guidance = max(self._after_guidance, item["id"])
                newest = item["instructions"]
        return newest

    async def run(self, advise: Callable[[str], Awaitable[None]]) -> None:
        """Reports turns as they come and hands guidance to the tutor. Runs until cancelled."""
        while True:
            await self._wake.wait()
            self._wake.clear()
            guidance = await self.report_once()
            if guidance:
                await advise(guidance)

    async def end(self) -> None:
        if self._flags:
            # Flags are best effort; the end of a session does not wait long for them.
            _, late = await asyncio.wait(self._flags, timeout=FLAG_TIMEOUT_SECONDS)
            for task in late:
                task.cancel()
            await asyncio.gather(*late, return_exceptions=True)
        if self._pending:
            await self.report_once()
        await self._call("POST", "/v1/agent/session/end", {}, REPORT_TIMEOUT_SECONDS)
        if self._http_session is not None:
            await self._http_session.close()
