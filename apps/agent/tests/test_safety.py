import asyncio

import pytest

from capricorn_agent.safety import Gates, audience_of


class Fixed:
    def __init__(self, verdict):
        self.verdict = verdict
        self.seen = []

    async def allows(self, text):
        self.seen.append(text)
        return self.verdict


class Broken:
    async def allows(self, text):
        raise RuntimeError("provider down")


class Slow:
    async def allows(self, text):
        await asyncio.sleep(5)
        return True


@pytest.mark.parametrize(
    ("metadata", "audience"),
    [
        ('{"audience": "operator"}', "operator"),
        ('{"audience": "child"}', "child"),
        ('{"audience": "OPERATOR"}', "child"),
        ("{}", "child"),
        ("", "child"),
        ("not json", "child"),
        ('["operator"]', "child"),
    ],
)
def test_only_an_explicit_operator_is_not_a_child(metadata, audience):
    assert audience_of(metadata) == audience


def test_a_child_session_needs_both_gates():
    assert not Gates().may_start("child")
    assert not Gates(moderation=Fixed(True)).may_start("child")
    assert not Gates(physical_safety=Fixed(True)).may_start("child")
    assert Gates(moderation=Fixed(True), physical_safety=Fixed(True)).may_start("child")


def test_an_operator_session_may_start_without_gates():
    assert Gates().may_start("operator")


async def test_what_is_heard_passes_both_gates():
    moderation, physical = Fixed(True), Fixed(True)
    gates = Gates(moderation=moderation, physical_safety=physical)

    assert await gates.allow_heard("voy a enchufar esto")
    assert moderation.seen == physical.seen == ["voy a enchufar esto"]


async def test_one_gate_saying_no_is_enough():
    assert not await Gates(moderation=Fixed(True), physical_safety=Fixed(False)).allow_heard("x")
    assert not await Gates(moderation=Fixed(False), physical_safety=Fixed(True)).allow_heard("x")


async def test_what_is_spoken_passes_moderation_only():
    physical = Fixed(False)
    gates = Gates(moderation=Fixed(True), physical_safety=physical)

    assert await gates.allow_spoken("muy bien")
    assert physical.seen == []


async def test_a_gate_that_fails_says_no():
    assert not await Gates(moderation=Broken(), physical_safety=Fixed(True)).allow_heard("x")


async def test_a_gate_that_is_late_says_no():
    gates = Gates(moderation=Slow(), physical_safety=Fixed(True), timeout_seconds=0.01)

    assert not await gates.allow_heard("x")


async def test_a_verdict_that_is_not_true_says_no():
    assert not await Gates(moderation=Fixed(None), physical_safety=Fixed("yes")).allow_heard("x")


async def test_without_gates_everything_passes():
    assert await Gates().allow_heard("x")
    assert await Gates().allow_spoken("x")
    assert not Gates().checks_spoken
