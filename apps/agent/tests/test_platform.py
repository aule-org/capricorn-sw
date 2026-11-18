import asyncio
import json

from capricorn_agent.platform import (
    FLAG_COOLDOWN_SECONDS,
    MAX_FLAGS_IN_FLIGHT,
    MAX_PENDING_TURNS,
    Platform,
    PlatformSession,
    platform_session,
)

SESSION = PlatformSession(url="http://platform", token="t")
ENV = {"CAPRICORN_PLATFORM_URL": "http://platform/"}


class Recorder:
    """Stands in for the platform: records calls and answers from a script."""

    def __init__(self, *answers):
        self.calls = []
        self.answers = list(answers)

    async def __call__(self, method, path, body, timeout):
        self.calls.append((method, path, body))
        answer = self.answers.pop(0) if self.answers else {}
        if isinstance(answer, Exception):
            raise answer
        return answer


def test_a_job_belongs_to_a_platform_session_only_with_a_url_and_a_token():
    metadata = json.dumps({"audience": "operator", "platform_token": "abc"})

    assert platform_session(metadata, ENV) == PlatformSession(url="http://platform", token="abc")
    assert platform_session(metadata, {}) is None
    assert platform_session('{"audience": "operator"}', ENV) is None
    assert platform_session('{"platform_token": 7}', ENV) is None
    assert platform_session("not json", ENV) is None


async def test_start_returns_the_instructions_or_nothing():
    assert await Platform(SESSION, Recorder({"instructions": "Plan"})).start() == "Plan"
    assert await Platform(SESSION, Recorder({})).start() == ""
    assert await Platform(SESSION, Recorder(RuntimeError("down"))).start() == ""


async def test_turns_are_numbered_and_named_for_the_platform():
    send = Recorder({"guidance": []})
    platform = Platform(SESSION, send)
    platform.heard("assistant", " ¿Cuánto es? ")
    platform.heard("user", "No sé")
    platform.heard("system", "ignored")
    platform.heard("user", "   ")

    assert await platform.report_once() is None

    assert send.calls == [
        (
            "POST",
            "/v1/agent/session/turns",
            {
                "turns": [
                    {"seq": 0, "role": "tutor", "content": "¿Cuánto es?"},
                    {"seq": 1, "role": "child", "content": "No sé"},
                ],
                "after_guidance": 0,
            },
        )
    ]


async def test_the_newest_guidance_is_returned_and_not_asked_for_again():
    send = Recorder(
        {"guidance": [{"id": 3, "instructions": "old"}, {"id": 4, "instructions": "new"}]},
        {"guidance": []},
    )
    platform = Platform(SESSION, send)
    platform.heard("user", "a")

    assert await platform.report_once() == "new"
    platform.heard("user", "b")
    assert await platform.report_once() is None
    assert send.calls[1][2]["after_guidance"] == 4


async def test_turns_survive_a_failed_report_up_to_a_limit():
    send = Recorder(RuntimeError("down"), {"guidance": []})
    platform = Platform(SESSION, send)
    platform.heard("user", "first")

    assert await platform.report_once() is None
    platform.heard("assistant", "second")
    await platform.report_once()

    assert [turn["content"] for turn in send.calls[1][2]["turns"]] == ["first", "second"]

    for n in range(MAX_PENDING_TURNS + 10):
        platform.heard("user", str(n))
    await platform.report_once()
    batches = [body["turns"] for _, _, body in send.calls[2:]]
    assert all(len(batch) <= 20 for batch in batches)
    assert sum(len(batch) for batch in batches) == MAX_PENDING_TURNS


async def test_ending_sends_what_is_left_and_then_closes_the_session():
    send = Recorder({"guidance": []}, {})
    platform = Platform(SESSION, send)
    platform.heard("user", "bye")

    await platform.end()

    assert [call[1] for call in send.calls] == ["/v1/agent/session/turns", "/v1/agent/session/end"]


async def test_backlogged_turns_fit_the_api_limit_before_ending():
    send = Recorder()
    platform = Platform(SESSION, send)
    for n in range(45):
        platform.heard("user", str(n))

    await platform.end()

    batches = [body["turns"] for _, path, body in send.calls if path.endswith("/turns")]
    assert [len(batch) for batch in batches] == [20, 20, 5]
    assert [turn["seq"] for batch in batches for turn in batch] == list(range(45))
    assert send.calls[-1][1] == "/v1/agent/session/end"


async def test_a_failed_report_is_retried_without_waiting_for_another_turn(monkeypatch):
    monkeypatch.setattr("capricorn_agent.platform.REPORT_RETRY_SECONDS", 0.01, raising=False)
    delivered = asyncio.Event()
    calls = []

    async def send(method, path, body, timeout):
        calls.append(body)
        if len(calls) == 1:
            raise RuntimeError("temporary outage")
        delivered.set()
        return {}

    async def advise(_):
        pass

    platform = Platform(SESSION, send)
    platform.heard("user", "retry this turn")
    worker = asyncio.create_task(platform.run(advise))
    try:
        await asyncio.wait_for(delivered.wait(), timeout=0.5)
        assert calls[1]["turns"] == calls[0]["turns"]
    finally:
        worker.cancel()
        await asyncio.gather(worker, return_exceptions=True)


async def test_cancelling_an_inflight_report_keeps_its_turn_for_shutdown():
    started = asyncio.Event()
    calls = []

    async def send(method, path, body, timeout):
        calls.append((path, body))
        if len(calls) == 1:
            started.set()
            await asyncio.Event().wait()
        return {}

    platform = Platform(SESSION, send)
    platform.heard("user", "last turn")
    reporting = asyncio.create_task(platform.report_once())
    await started.wait()
    reporting.cancel()
    await asyncio.gather(reporting, return_exceptions=True)
    await platform.end()

    assert [path for path, _ in calls] == [
        "/v1/agent/session/turns",
        "/v1/agent/session/turns",
        "/v1/agent/session/end",
    ]
    assert calls[1][1]["turns"] == calls[0][1]["turns"]


async def test_shutdown_retries_a_failed_flush_before_ending(monkeypatch):
    monkeypatch.setattr("capricorn_agent.platform.REPORT_RETRY_SECONDS", 0.01)
    send = Recorder(RuntimeError("temporary outage"), {}, {})
    platform = Platform(SESSION, send)
    platform.heard("user", "last turn")

    await platform.end()

    assert [call[1] for call in send.calls] == [
        "/v1/agent/session/turns",
        "/v1/agent/session/turns",
        "/v1/agent/session/end",
    ]
    assert send.calls[-1][2] == {}


async def test_shutdown_bounds_an_outage_and_reports_the_loss(monkeypatch, caplog):
    monkeypatch.setattr("capricorn_agent.platform.REPORT_RETRY_SECONDS", 0.01)
    monkeypatch.setattr("capricorn_agent.platform.END_FLUSH_TIMEOUT_SECONDS", 0.03)
    calls = []

    async def send(method, path, body, timeout):
        calls.append((path, body))
        if path.endswith("/turns"):
            raise RuntimeError("outage")
        return {}

    platform = Platform(SESSION, send)
    platform.heard("user", "private text that must not be logged")
    await asyncio.wait_for(platform.end(), timeout=0.5)

    assert calls[-1] == ("/v1/agent/session/end", {"end_reason": "error"})
    assert "session ended with unreported turns" in caplog.text
    assert "private text" not in caplog.text


FLAGS = "/v1/agent/session/flags"


async def settle(platform):
    await asyncio.gather(*platform._flags)


async def test_a_flag_is_posted_in_the_background_with_fixed_vocabulary_only():
    release = asyncio.Event()
    calls = []

    async def send(method, path, body, timeout):
        calls.append((method, path, body))
        await release.wait()
        return {}

    platform = Platform(SESSION, send)

    platform.flag("physical_safety", "adult_help", ["physical_hazard", "violence"])  # no await

    await asyncio.sleep(0)
    assert calls == [
        (
            "POST",
            FLAGS,
            {
                "gate": "physical_safety",
                "action": "adult_help",
                "categories": ["physical_hazard", "violence"],
            },
        )
    ]
    release.set()
    await settle(platform)


async def test_a_flag_outside_the_vocabulary_is_dropped_without_raising():
    send = Recorder()
    platform = Platform(SESSION, send)

    platform.flag("moderation_input", "redirect", ["what the child said"])
    platform.flag("moderation_input", "redirect", [])
    platform.flag("moderation_input", "scold", ["violence"])
    platform.flag("abuse_limit", "redirect", ["violence"])
    platform.flag("the microphone", "redirect", ["violence"])

    assert platform._flags == set()
    assert send.calls == []


async def test_one_topic_is_flagged_once_per_cooldown():
    now = [1000.0]
    send = Recorder()
    platform = Platform(SESSION, send, clock=lambda: now[0])

    def flag(gate="moderation_input", action="redirect", categories=("violence",)):
        platform.flag(gate, action, list(categories))

    flag()
    flag()
    flag(categories=("violence", "hate"))  # the same gravest category
    await settle(platform)
    assert len(send.calls) == 1

    # Another category, action or gate is another topic: a quiet flag never
    # hides a grave one.
    flag(categories=("self_harm", "violence"), action="support")
    flag(gate="moderation_output")
    flag(gate="physical_safety", action="clarify", categories=("physical_hazard",))
    flag(gate="physical_safety", action="adult_help", categories=("physical_hazard",))
    await settle(platform)
    assert len(send.calls) == 5

    now[0] += FLAG_COOLDOWN_SECONDS - 1
    flag()
    await settle(platform)
    assert len(send.calls) == 5

    now[0] += 1
    flag()
    await settle(platform)
    assert len(send.calls) == 6


async def test_a_flag_that_did_not_arrive_does_not_use_up_the_cooldown():
    send = Recorder(RuntimeError("down"), {})
    platform = Platform(SESSION, send)

    platform.flag("moderation_input", "support", ["self_harm"])
    await settle(platform)
    platform.flag("moderation_input", "support", ["self_harm"])
    await settle(platform)

    assert [call[1] for call in send.calls] == [FLAGS, FLAGS]


async def test_flags_in_flight_are_bounded_and_the_end_does_not_wait_for_them(monkeypatch):
    monkeypatch.setattr("capricorn_agent.platform.FLAG_TIMEOUT_SECONDS", 0.01)
    calls = []

    async def send(method, path, body, timeout):
        calls.append(path)
        if path == FLAGS:
            await asyncio.Event().wait()
        return {}

    platform = Platform(SESSION, send)
    categories = ["self_harm", "sexual", "violence", "harassment", "hate", "illicit"]
    for category in categories:
        platform.flag("moderation_input", "redirect", [category])
    await asyncio.sleep(0)
    assert calls == [FLAGS] * MAX_FLAGS_IN_FLIGHT

    await platform.end()

    assert calls[-1] == "/v1/agent/session/end"
    assert all(task.cancelled() for task in platform._flags)
