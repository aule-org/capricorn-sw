import asyncio
import time

from aule_agent.camera import Camera


async def test_a_fresh_frame_is_one_that_arrives_after_it_was_asked_for():
    camera = Camera(room=None)
    # What the camera saw before the student pointed it at their work.
    camera._frame, camera._frame_at = "the ceiling", time.monotonic() - 1

    waiting = asyncio.create_task(camera.fresh(1.0))
    await asyncio.sleep(0.1)
    assert not waiting.done()
    camera._frame, camera._frame_at = "the notebook", time.monotonic()

    assert await waiting == "the notebook"


async def test_a_camera_that_sends_nothing_gives_up_in_time():
    camera = Camera(room=None)
    camera._frame, camera._frame_at = "the ceiling", time.monotonic()
    started = time.monotonic()

    assert await camera.fresh(0.2) is None
    assert 0.2 <= time.monotonic() - started < 0.5
