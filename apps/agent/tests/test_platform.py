import json

from capricorn_agent.platform import MAX_PENDING_TURNS, Platform, PlatformSession, platform_session

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
    assert len(send.calls[2][2]["turns"]) == MAX_PENDING_TURNS


async def test_ending_sends_what_is_left_and_then_closes_the_session():
    send = Recorder({"guidance": []}, {})
    platform = Platform(SESSION, send)
    platform.heard("user", "bye")

    await platform.end()

    assert [call[1] for call in send.calls] == ["/v1/agent/session/turns", "/v1/agent/session/end"]
