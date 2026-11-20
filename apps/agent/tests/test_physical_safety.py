"""The physical-safety gate with a fake transport: no network, no paid calls."""

import asyncio
import json

import pytest
from livekit.agents import llm

from aule_agent.moderation import Action, Decision
from aule_agent.physical_safety import (
    MAX_BYTES,
    MAX_HISTORY_ITEMS,
    QUESTION,
    PhysicalSafetyGuard,
    Verdict,
    combine_input_decisions,
    parse_response,
)


def message(text="Hola", *, role="user"):
    return llm.ChatMessage(role=role, content=[text])


def response(probability=0.01):
    return {
        "model": "jev-1.14.0",
        "answers": {"needs_adult": {"type": "noul", "noul": probability}},
        "usage": {"input_tokens": 700},
    }


class Provider:
    """Stands in for the provider: records request bodies and answers from a script."""

    def __init__(self):
        self.requests = []
        self.payload = response()
        self.raw = None
        self.error = None
        self.delay = 0.0

    async def __call__(self, body):
        assert len(body) <= MAX_BYTES
        self.requests.append(json.loads(body))
        await asyncio.sleep(self.delay)
        if self.error is not None:
            raise self.error
        return self.raw if self.raw is not None else json.dumps(self.payload).encode()


@pytest.fixture
def provider():
    return Provider()


@pytest.fixture
async def guard(provider):
    instance = PhysicalSafetyGuard("synthetic-test-key", age=8, send=provider)
    yield instance
    await instance.close()


def empty():
    return llm.ChatContext.empty()


@pytest.mark.parametrize(
    ("risk", "verdict"),
    [
        (0, Verdict.ALLOW),
        (0.1, Verdict.ALLOW),
        (0.10001, Verdict.UNCERTAIN),
        (0.24999, Verdict.UNCERTAIN),
        (0.25, Verdict.ADULT_HELP),
        (1, Verdict.ADULT_HELP),
    ],
)
def test_probability_boundaries(risk, verdict):
    assert parse_response(json.dumps(response(risk)).encode()) is verdict


@pytest.mark.parametrize("risk", [True, None, "0.0", -0.1, 1.1, float("nan"), float("inf")])
def test_an_invalid_probability_never_allows(risk):
    with pytest.raises(ValueError):
        parse_response(json.dumps(response(risk)).encode())


@pytest.mark.parametrize("verdict", list(Verdict))
@pytest.mark.parametrize("action", list(Action))
def test_combining_never_weakens_what_moderation_decided(action, verdict):
    existing = Decision(action, None if action is Action.ALLOW else "original")

    combined = combine_input_decisions(existing, verdict)

    if action in (Action.SUPPORT, Action.UNAVAILABLE) or verdict is Verdict.ALLOW:
        assert combined == existing
    elif verdict is Verdict.ADULT_HELP:
        assert combined == Decision(Action.REDIRECT, "adult_help")
    elif verdict is Verdict.UNCERTAIN:
        assert combined == (
            existing if action is Action.REDIRECT else Decision(Action.REDIRECT, "clarify")
        )
    elif verdict is Verdict.CONTEXT_LIMIT:
        assert combined == Decision(Action.UNAVAILABLE, "context_limit")
    else:
        assert combined == Decision(Action.UNAVAILABLE, "unavailable")
    assert not (combined.allowed and (verdict is not Verdict.ALLOW or not existing.allowed))


async def test_a_closed_guard_answers_unavailable_without_calling(guard, provider):
    assert await guard.check(empty(), message()) is Verdict.ALLOW
    assert provider.requests[0]["model"] == "jev-1.14.0"

    await guard.close()

    assert await guard.check(empty(), message()) is Verdict.UNAVAILABLE
    assert len(provider.requests) == 1


@pytest.mark.parametrize(
    "error", [ValueError("unexpected status"), OSError("network"), RuntimeError("anything")]
)
async def test_a_failed_call_is_unavailable_and_not_retried(guard, provider, error):
    provider.error = error

    assert await guard.check(empty(), message()) is Verdict.UNAVAILABLE
    assert len(provider.requests) == 1


@pytest.mark.parametrize(
    "raw",
    [
        b"not json",
        b"null",
        b"[]",
        b'{"model":"other","answers":{}}',
        b'{"model":"jev-1.14.0","answers":{"needs_adult":{"type":"choice","noul":0}}}',
        b'{"model":"jev-1.14.0","answers":{"needs_adult":{"type":"noul","noul":0},"extra":{}}}',
        b"[" * 2000 + b"0" + b"]" * 2000,
    ],
)
async def test_a_malformed_answer_fails_closed(guard, provider, raw):
    provider.raw = raw

    assert await guard.check(empty(), message()) is Verdict.UNAVAILABLE


@pytest.mark.parametrize("model", ["jev-1.13.0", "jev-preview", "jev-latest", "other"])
async def test_an_answer_from_another_model_cannot_allow(guard, provider, model):
    provider.payload["model"] = model

    assert await guard.check(empty(), message()) is Verdict.UNAVAILABLE


async def test_a_late_answer_is_unavailable_and_not_retried(guard, provider, monkeypatch):
    monkeypatch.setattr("aule_agent.physical_safety.TIMEOUT_SECONDS", 0.01)
    provider.delay = 5

    assert await guard.check(empty(), message()) is Verdict.UNAVAILABLE
    assert len(provider.requests) == 1


async def test_only_recent_text_is_sent_and_the_conversation_is_left_alone(guard, provider):
    context = empty()
    context.add_message(role="system", content="Private system prompt")
    context.add_message(role="developer", content="Private developer prompt")
    context.items.append(
        llm.FunctionCall(id="tool", call_id="t", name="lookup", arguments="secret")
    )
    original = list(context.items)
    provider.payload = response(0.99)
    first = message("Tengo un cuchillo real en la mano")

    assert await guard.check(context, first) is Verdict.ADULT_HELP
    await guard.check(context, message("¿Se lo llevo?"))

    # The stopped turn never joined the conversation, yet the follow-up has it.
    sent = provider.requests[-1]
    assert sent["state"] == {
        "age": 8,
        "history": [{"role": "user", "content": "Tengo un cuchillo real en la mano"}],
        "utterance": "¿Se lo llevo?",
    }
    assert sent["questions"] == {"needs_adult": QUESTION}
    assert context.items == original
    assert "secret" not in json.dumps(sent)
    assert "Private" not in json.dumps(sent)


async def test_history_is_ordered_without_repeats_and_bounded(provider):
    guard = PhysicalSafetyGuard("test", age=True, send=provider)
    context = empty()
    previous = message("Ya viene mi mamá")
    assert await guard.check(context, previous) is Verdict.ALLOW
    context.items.append(previous)
    context.add_message(role="assistant", content="Pídele ayuda")

    assert await guard.check(context, message("Está aquí")) is Verdict.ALLOW

    sent = provider.requests[-1]
    assert sent["state"]["age"] == "unknown (child)"
    assert [item["content"] for item in sent["state"]["history"]] == [
        "Ya viene mi mamá",
        "Pídele ayuda",
    ]

    for number in range(100):
        context.add_message(role="user", content=str(number))
    assert await guard.check(context, message("Hola")) is Verdict.ALLOW
    assert len(provider.requests[-1]["state"]["history"]) == MAX_HISTORY_ITEMS


async def test_a_sentence_too_long_to_judge_is_never_sent_or_forgotten(guard, provider):
    assert await guard.check(empty(), message("ñ" * MAX_BYTES)) is Verdict.CONTEXT_LIMIT
    assert await guard.check(empty(), message("¿Lo hago?")) is Verdict.CONTEXT_LIMIT
    assert provider.requests == []


async def test_old_context_is_dropped_as_whole_messages(guard, provider):
    context = empty()
    for text in ("a" * 10000, "ñ" * 6000, "c" * 10000):
        context.add_message(role="user", content=text)
    original = list(context.items)

    assert await guard.check(context, message()) is Verdict.ALLOW

    history = provider.requests[-1]["state"]["history"]
    assert [item["content"] for item in history] == ["ñ" * 6000, "c" * 10000]
    assert context.items == original


async def test_a_long_conversation_does_not_block_later_short_turns(guard, provider):
    context = empty()
    context.add_message(role="user", content="a" * 11000)
    context.add_message(role="assistant", content="b" * 1000)
    current = message("c" * 10000)
    assert await guard.check(context, current) is Verdict.ALLOW
    context.items.append(current)
    context.add_message(role="assistant", content="d" * 1000)

    for _ in range(5):
        assert await guard.check(context, message("Hola")) is Verdict.ALLOW
        state = provider.requests[-1]["state"]
        assert state["utterance"] == "Hola"
        assert {item["content"] for item in state["history"]} >= {"c" * 10000, "d" * 1000}


@pytest.mark.parametrize(
    ("probability", "error", "verdict"),
    [
        (0.99, None, Verdict.ADULT_HELP),
        (0.2, None, Verdict.UNCERTAIN),
        (0.01, ValueError("unexpected status"), Verdict.UNAVAILABLE),
    ],
)
async def test_unresolved_context_outlives_the_history_window(
    guard, provider, probability, error, verdict
):
    context = empty()
    context.add_message(role="assistant", content="¿Qué tienes en la mano?")
    provider.payload, provider.error = response(probability), error
    assert await guard.check(context, message("Tengo un cuchillo real")) is verdict

    context = empty()  # the conversation may have been trimmed since
    for number in range(30):
        context.add_message(role="assistant", content=f"{number}:" + "x" * 1000)
    provider.payload, provider.error = response(0.01), None
    assert await guard.check(context, message("¿Se lo llevo?")) is Verdict.ALLOW

    contents = [item["content"] for item in provider.requests[-1]["state"]["history"]]
    assert contents[:2] == ["¿Qué tienes en la mano?", "Tengo un cuchillo real"]

    assert not guard._protected  # the "allow" resolved it


async def test_unresolved_context_that_no_longer_fits_stops_the_session(guard, provider):
    provider.payload = response(0.99)

    assert await guard.check(empty(), message("a" * 15000)) is Verdict.ADULT_HELP
    assert await guard.check(empty(), message("b" * 10000)) is Verdict.CONTEXT_LIMIT
    assert await guard.check(empty(), message("¿Lo hago?")) is Verdict.CONTEXT_LIMIT
    assert len(provider.requests) == 1


async def test_a_cancelled_check_propagates_and_keeps_its_context(guard, provider):
    provider.delay = 0.3
    task = asyncio.create_task(guard.check(empty(), message("Tengo un cable pelado")))
    while not provider.requests:
        await asyncio.sleep(0)
    task.cancel()
    with pytest.raises(asyncio.CancelledError):
        await task

    provider.delay = 0
    await guard.check(empty(), message("¿Lo toco?"))

    assert provider.requests[-1]["state"]["history"] == [
        {"role": "user", "content": "Tengo un cable pelado"}
    ]


async def test_logs_carry_the_verdict_and_never_the_text_or_the_key(guard, provider, caplog):
    provider.payload = response(0.99)

    with caplog.at_level("DEBUG"):
        await guard.check(empty(), message("secret words of a child"))

    (record,) = caplog.records
    assert record.verdict == "adult_help"
    assert isinstance(record.duration_ms, int)
    logged = str(record.__dict__)
    assert "secret" not in logged and "synthetic-test-key" not in logged and "0.99" not in logged
