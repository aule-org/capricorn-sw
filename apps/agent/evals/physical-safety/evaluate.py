"""Replays synthetic conversations against the physical-safety gate.

Without ``--live`` it only prints what would be run and makes no calls. With
``--live`` it calls the real provider, which costs money and sends the case
texts to it: run it by hand, never in CI. The key is read from stdin so it
never appears in a command line.

    uv run python evals/physical-safety/evaluate.py
    uv run python evals/physical-safety/evaluate.py --live < key.txt
"""

import argparse
import asyncio
import hashlib
import json
import math
import statistics
import sys
import time
from collections import Counter
from pathlib import Path

from livekit.agents import llm

from capricorn_agent import physical_safety as gate
from capricorn_agent.replies import ADULT_HELP_SEQUENCE, reply

MAX_REPETITIONS = 3


def emit(value: dict) -> None:
    print(json.dumps(value, ensure_ascii=False), flush=True)


async def evaluate(args: argparse.Namespace) -> int:
    fixture = args.cases.read_bytes()
    scenarios = json.loads(fixture)["scenarios"]
    turns = [turn for case in scenarios for turn in case["turns"]]
    assert 1 <= args.repetitions <= MAX_REPETITIONS
    assert all(turn["expected"] in {"allow", "adult_help"} for turn in turns)
    assert all(turn.get("language", "es") in {"es", "en"} for turn in turns)
    expected_checks = len(turns) * args.repetitions
    emit(
        {
            "kind": "manifest",
            "model": gate.MODEL,
            "expected_checks": expected_checks,
            "max_checks": args.max_checks,
            "timeout_seconds": gate.TIMEOUT_SECONDS,
            "question_sha256": hashlib.sha256(
                json.dumps(gate.QUESTION, sort_keys=True).encode()
            ).hexdigest(),
            "source_sha256": hashlib.sha256(Path(gate.__file__).read_bytes()).hexdigest(),
            "fixture_sha256": hashlib.sha256(fixture).hexdigest(),
            "live": args.live,
        }
    )
    if not args.live:
        return 0
    if expected_checks > args.max_checks:
        emit({"kind": "summary", "status": "over_max_checks", "pass": False, "completed": 0})
        return 1
    key = sys.stdin.read().strip()
    assert key and not any(char.isspace() for char in key)

    # One connection for the whole run, as within a session; each scenario
    # still gets a guard of its own, so no memory is shared between them.
    connection = gate.PhysicalSafetyGuard(key)
    rows = []
    try:
        for repetition in range(1, args.repetitions + 1):
            for case in scenarios:
                rows += await replay(
                    case,
                    repetition,
                    gate.PhysicalSafetyGuard(key, age=case["age"], send=connection._http),
                )
    finally:
        await connection.close()

    times = sorted(row["latency_ms"] for row in rows)
    passed = len(rows) == expected_checks and all(row["pass"] for row in rows)
    emit(
        {
            "kind": "summary",
            "status": "complete",
            "pass": passed,
            "completed": len(rows),
            "matched": sum(row["pass"] for row in rows),
            "verdicts": dict(Counter(row["verdict"] for row in rows)),
            "latency_ms": {
                "median": statistics.median(times),
                "p95": times[math.ceil(len(times) * 0.95) - 1],
                "max": max(times),
            }
            if times
            else None,
        }
    )
    return 0 if passed else 1


async def replay(case: dict, repetition: int, guard: gate.PhysicalSafetyGuard) -> list[dict]:
    """Runs one scenario the way a session would, and returns one row per turn."""
    context = llm.ChatContext.empty()
    for item in case.get("history", []):
        context.add_message(role=item["role"], content=item["content"])
    rows = []
    hazards = 0
    try:
        for index, turn in enumerate(case["turns"]):
            message = llm.ChatMessage(role="user", content=[turn["text"]])
            started = time.monotonic()
            verdict = await guard.check(context, message)
            row = {
                "kind": "check",
                "id": case["id"],
                "rep": repetition,
                "turn": index + 1,
                "expected": turn["expected"],
                "verdict": verdict.value,
                "pass": verdict.value == turn["expected"],
                "latency_ms": (time.monotonic() - started) * 1000,
            }
            rows.append(row)
            emit(row)
            # As in a session: an allowed turn joins the conversation; a stopped
            # one stays private to the guard and only the fixed offer is added.
            language = turn.get("language", "es")
            if verdict is gate.Verdict.ALLOW:
                context.items.append(message)
                hazards = 0
            elif verdict is gate.Verdict.ADULT_HELP:
                context.add_message(
                    role="assistant", content=reply(ADULT_HELP_SEQUENCE[hazards], language)
                )
                hazards = hazards + 1 if hazards + 1 < len(ADULT_HELP_SEQUENCE) else 1
            elif verdict is gate.Verdict.UNCERTAIN:
                context.add_message(role="assistant", content=reply("clarify", language))
                hazards = 0
    finally:
        await guard.close()
    return rows


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--live", action="store_true", help="call the real provider")
    parser.add_argument("--repetitions", type=int, default=2)
    parser.add_argument("--max-checks", type=int, default=200, help="refuse a larger live run")
    parser.add_argument(
        "--cases", type=Path, default=Path(__file__).with_name("turn-recovery-cases.json")
    )
    sys.exit(asyncio.run(evaluate(parser.parse_args())))
