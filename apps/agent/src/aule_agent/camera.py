"""The latest frame of the person's camera, so the tutor can look at their work.

Frames are held in memory one at a time and never stored: a new frame replaces
the previous one, and a frame older than a few seconds (camera off or muted)
is not used.
"""

import asyncio
import time

from livekit import rtc

MAX_AGE_SECONDS = 3.0


class Camera:
    def __init__(self, room: rtc.Room) -> None:
        self._room = room
        self._frame: rtc.VideoFrame | None = None
        self._frame_at = 0.0
        self._reader: asyncio.Task[None] | None = None

    def start(self) -> None:
        self._room.on("track_subscribed", self._on_track_subscribed)
        for participant in self._room.remote_participants.values():
            for publication in participant.track_publications.values():
                if publication.track is not None:
                    self._on_track_subscribed(publication.track, publication, participant)

    def stop(self) -> None:
        self._room.off("track_subscribed", self._on_track_subscribed)
        if self._reader is not None:
            self._reader.cancel()
        self._frame = None

    def latest(self) -> rtc.VideoFrame | None:
        if self._frame is None or time.monotonic() - self._frame_at > MAX_AGE_SECONDS:
            return None
        return self._frame

    def _on_track_subscribed(
        self,
        track: rtc.Track,
        publication: rtc.RemoteTrackPublication,
        participant: rtc.RemoteParticipant,
    ) -> None:
        if publication.source != rtc.TrackSource.SOURCE_CAMERA:
            return
        if self._reader is not None:
            self._reader.cancel()
        self._reader = asyncio.create_task(self._read(track))

    async def _read(self, track: rtc.Track) -> None:
        stream = rtc.VideoStream(track, capacity=1)
        try:
            async for event in stream:
                self._frame = event.frame
                self._frame_at = time.monotonic()
        finally:
            self._frame = None
            await stream.aclose()
