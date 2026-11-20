import asyncio

import pytest

from aule_agent.pace import BREAK_COOLDOWN_SECONDS, QUIET_TIMEOUT_SECONDS, Pace, warning
from aule_agent.replies import BREAKS, REPLIES


class Time:
    """A clock that only moves when somebody sleeps, so an hour takes no time."""

    def __init__(self):
        self.now = 0.0

    def clock(self):
        return self.now

    async def sleep(self, seconds):
        self.now += seconds
        await asyncio.sleep(0)


class Session:
    """What the pace needs of a voice session: who is talking, and a voice."""

    def __init__(self, time):
        self.time = time
        self.agent_state = "listening"
        self.user_state = "listening"
        self.said = []
        self.closed_at = None

    async def say(self, text, *, add_to_chat_ctx, allow_interruptions):
        self.said.append((self.time.now, text, add_to_chat_ctx, allow_interruptions))

    def shutdown(self):
        self.closed_at = self.time.now


def pace(minutes=60, break_minutes=5, taken=None):
    time = Time()
    session = Session(time)
    made = Pace(
        session,
        "es",
        max_minutes=minutes,
        break_minutes=break_minutes,
        taken=taken,
        clock=time.clock,
        sleep=time.sleep,
    )
    return made, session, time


def line(key):
    return REPLIES[key]["es"]


@pytest.mark.parametrize(
    ("minutes", "expected"),
    [
        (60, (5, "five_minutes_left")),
        (10, (5, "five_minutes_left")),
        (9, (2, "two_minutes_left")),
        (3, (2, "two_minutes_left")),
        (2, None),
        (1, None),
    ],
)
def test_the_end_is_announced_five_minutes_before_or_two_in_a_short_session(minutes, expected):
    assert warning(minutes) == expected


async def test_a_session_is_warned_and_then_said_goodbye_to_and_closed():
    made, session, _ = pace(minutes=60)

    await made.run()

    assert session.said == [
        # The warning stays in the conversation, so the tutor knows; the
        # goodbye does not. Neither can be talked over.
        (55 * 60.0, line("five_minutes_left"), True, False),
        (60 * 60.0, line("goodbye"), False, False),
    ]
    assert session.closed_at == 60 * 60.0
    assert made.minutes() == 60


async def test_a_session_too_short_for_a_warning_still_gets_its_goodbye():
    made, session, _ = pace(minutes=2)

    await made.run()

    assert [text for _, text, _, _ in session.said] == [line("goodbye")]


async def test_nothing_is_said_over_the_tutor_or_the_student():
    made, session, time = pace(minutes=10)
    session.agent_state = "speaking"

    async def finish_the_reply():
        # The tutor is mid-reply when the warning is due, then the student
        # answers at once; the warning waits for both.
        while time.now < 5 * 60 + 4:
            await asyncio.sleep(0)
        session.agent_state, session.user_state = "listening", "speaking"
        while time.now < 5 * 60 + 7:
            await asyncio.sleep(0)
        session.user_state = "listening"

    await asyncio.gather(made.run(), finish_the_reply())

    warned_at = session.said[0][0]
    assert 5 * 60 + 7 <= warned_at < 5 * 60 + 8
    # The goodbye is still on time: the wait does not push the end back.
    assert session.closed_at == 10 * 60.0


async def test_a_line_is_not_held_back_for_ever_by_a_session_that_never_goes_quiet():
    made, session, _ = pace(minutes=10)
    session.agent_state = "thinking"

    await made.run()

    assert session.said[0][0] == pytest.approx(5 * 60 + QUIET_TIMEOUT_SECONDS)


async def test_a_break_proposes_one_activity_and_asks_to_go_on_when_it_is_over():
    counted = []
    made, session, time = pace(break_minutes=5, taken=counted.append)
    time.now = 30 * 60.0

    await made.take_a_break()
    # Offered again while the break is on, as the platform does: nothing new.
    await made.take_a_break()
    for _ in range(5):
        await asyncio.sleep(0)

    assert counted == [1] and made.breaks_taken == 1
    assert session.said == [
        (30 * 60.0, line(BREAKS[0]), True, False),
        # Nobody talked for the whole break: the tutor asks, and can be answered over.
        (35 * 60.0, line("break_over"), True, True),
    ]


async def test_a_student_who_talks_ends_the_break_and_is_not_asked_to_go_on():
    made, session, time = pace(break_minutes=5)

    await made.take_a_break()
    time.now += 90
    made.student_spoke()
    await time.sleep(10 * 60)
    for _ in range(5):
        await asyncio.sleep(0)

    assert [text for _, text, _, _ in session.said] == [line(BREAKS[0])]


async def test_a_break_just_taken_is_not_offered_again_and_the_next_one_is_another_activity():
    made, session, time = pace()

    await made.take_a_break()
    made.student_spoke()
    time.now += BREAK_COOLDOWN_SECONDS - 1
    await made.take_a_break()
    assert made.breaks_taken == 1

    time.now += 2
    await made.take_a_break()
    made.close()

    assert made.breaks_taken == 2
    assert [text for _, text, _, _ in session.said] == [line(BREAKS[0]), line(BREAKS[1])]


def test_there_is_a_line_for_every_break_and_they_alternate_waking_and_calming():
    assert set(BREAKS) <= set(REPLIES)
    assert BREAKS == ("break_stars", "break_balloon", "break_mirror", "break_cat")
