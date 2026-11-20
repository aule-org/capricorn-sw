import asyncio
from types import SimpleNamespace

import pytest

from aule_agent.opinion import MODEL, SecondOpinion


class FakeClient:
    def __init__(self, answer="SAFE", delay=0.0):
        self.answer, self.delay, self.calls, self.closed = answer, delay, [], False
        self.messages = SimpleNamespace(create=self.create)

    async def create(self, **kwargs):
        self.calls.append(kwargs)
        await asyncio.sleep(self.delay)
        if isinstance(self.answer, Exception):
            raise self.answer
        return SimpleNamespace(
            content=[
                SimpleNamespace(type="thinking", thinking="SAFE"),
                SimpleNamespace(type="text", text=self.answer),
            ]
        )

    async def close(self):
        self.closed = True


async def test_only_the_word_safe_is_a_yes():
    assert await SecondOpinion(client=FakeClient(" safe\n")).safe("cuenta las canicas")
    for answer in ("UNSAFE", "SAFE, I think", "", "yes"):
        assert not await SecondOpinion(client=FakeClient(answer)).safe("x")


async def test_the_reply_is_sent_alone_to_the_small_model():
    client = FakeClient()

    await SecondOpinion(client=client).safe("cuenta las canicas")

    assert client.calls[0]["model"] == MODEL
    assert client.calls[0]["messages"] == [{"role": "user", "content": "cuenta las canicas"}]


async def test_no_opinion_is_not_a_yes(monkeypatch):
    monkeypatch.setattr("aule_agent.opinion.TIMEOUT_SECONDS", 0.01)

    assert not await SecondOpinion(client=FakeClient(delay=1)).safe("x")
    assert not await SecondOpinion(client=FakeClient(RuntimeError("down"))).safe("x")


async def test_cancelling_is_not_swallowed_and_the_client_is_closed_with_it():
    client = FakeClient(delay=1)
    opinion = SecondOpinion(client=client)
    task = asyncio.create_task(opinion.safe("x"))
    await asyncio.sleep(0.01)
    task.cancel()

    with pytest.raises(asyncio.CancelledError):
        await task
    await opinion.close()
    assert client.closed
