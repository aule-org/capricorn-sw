"""The LiveKit Agents worker: one tutor session per room it is dispatched to.

Run it with ``python -m capricorn_agent.worker start`` (``dev`` while developing).
The worker only joins rooms that ask for it by name (explicit dispatch).
"""

import asyncio
import json
import logging
import os
import sys

from livekit.agents import AgentServer, AgentSession, JobContext, JobProcess, cli, room_io
from livekit.plugins import silero

from .camera import Camera
from .config import LANGUAGES, ConfigError, Settings
from .platform import Platform, platform_session
from .providers import build_llm, build_stt, build_tts
from .safety import Gates, audience_of
from .tutor import Tutor

logger = logging.getLogger("capricorn.agent")

GOODBYE = {
    "es": "Se nos acabó el tiempo por hoy. ¡Buen trabajo! Hasta la próxima.",
    "en": "That is all our time for today. Good work! See you next time.",
}

# The decorator needs the name at import time; Settings validates the rest.
AGENT_NAME = os.environ.get("CAPRICORN_AGENT_NAME", "").strip() or "capricorn-tutor"


def language_of(dispatch_metadata: str, default: str) -> str:
    try:
        metadata = json.loads(dispatch_metadata or "{}")
    except ValueError:
        return default
    language = metadata.get("language") if isinstance(metadata, dict) else None
    return language if language in LANGUAGES else default


def prewarm(proc: JobProcess) -> None:
    proc.userdata["vad"] = silero.VAD.load()


# One warm process is enough for a small server; each holds the VAD model.
# The worker's health endpoint stays on loopback unless told otherwise.
server = AgentServer(
    setup_fnc=prewarm,
    num_idle_processes=1,
    host=os.environ.get("CAPRICORN_HEALTH_HOST", "127.0.0.1"),
    port=int(os.environ.get("CAPRICORN_HEALTH_PORT", "8081")),
)


@server.rtc_session(agent_name=AGENT_NAME)
async def entrypoint(ctx: JobContext) -> None:
    settings = Settings.from_env()
    audience = audience_of(ctx.job.metadata)
    language = language_of(ctx.job.metadata, settings.language)

    # No gate is implemented yet (see safety.py), so this refuses every session
    # that is not an operator's.
    gates = Gates()
    if not gates.may_start(audience):
        logger.warning("session refused: the safety gates are not configured")
        ctx.shutdown(reason="safety gates not configured")
        return

    await ctx.connect()
    camera = Camera(ctx.room)

    # With a platform, the tutor starts from what is known about the student
    # and reports the conversation; without one it is a tutor with no memory.
    linked = platform_session(ctx.job.metadata)
    platform = Platform(linked) if linked else None
    context = await platform.start() if platform else ""
    tutor = Tutor(language=language, gates=gates, camera=camera, context=context)
    session = AgentSession(
        stt=build_stt(settings, language),
        llm=build_llm(settings),
        tts=build_tts(settings, language),
        vad=ctx.proc.userdata["vad"],
    )

    async def end_when_time_is_up() -> None:
        await asyncio.sleep(settings.session_max_minutes * 60)
        await session.say(GOODBYE[language], allow_interruptions=False, add_to_chat_ctx=False)
        session.shutdown()

    tasks = [asyncio.create_task(end_when_time_is_up())]
    if platform:

        @session.on("conversation_item_added")
        def report(event) -> None:
            if getattr(event.item, "type", None) == "message":
                platform.heard(event.item.role, event.item.text_content)

        tasks.append(asyncio.create_task(platform.run(tutor.advise)))

    async def cleanup() -> None:
        for task in tasks:
            task.cancel()
        camera.stop()
        if platform:
            await platform.end()

    ctx.add_shutdown_callback(cleanup)

    await session.start(
        agent=tutor,
        room=ctx.room,
        # The camera is read by ``Camera``; the session's own video input only
        # feeds realtime models. Audio is not recorded.
        room_options=room_io.RoomOptions(video_input=False, delete_room_on_close=True),
        record=False,
    )
    camera.start()
    logger.info("session started", extra={"audience": audience, "language": language})


def main() -> None:
    # Checked here too so a misconfigured worker exits with one clear message
    # instead of registering and failing on its first session.
    if len(sys.argv) > 1 and sys.argv[1] in {"start", "dev"}:
        try:
            Settings.from_env()
        except ConfigError as error:
            sys.exit(f"capricorn-agent: {error}")
    cli.run_app(server)


if __name__ == "__main__":
    main()
