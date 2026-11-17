import json

import pytest
from jsonschema import ValidationError

from capricorn_agent.events import build_event, contracts_dir, validate_event

EXAMPLES = sorted((contracts_dir() / "examples").glob("*.json"))


@pytest.mark.parametrize("path", EXAMPLES, ids=lambda path: path.stem)
def test_examples_validate(path):
    validate_event(json.loads(path.read_text(encoding="utf-8")))


def test_there_are_examples():
    assert EXAMPLES


def _session_ended() -> dict:
    return json.loads((contracts_dir() / "examples/session.ended.v1.json").read_text())["data"]


def test_build_event_wraps_data_in_a_valid_envelope():
    data = _session_ended()
    event = build_event("session.ended.v1", data, subject=data["session_id"])
    assert event["source"] == "capricorn/agent"
    assert event["type"] == "session.ended.v1"
    assert event["subject"] == data["session_id"]
    assert event["time"].endswith("Z")
    validate_event(event)


def test_build_event_gives_each_event_its_own_id():
    data = _session_ended()
    assert (
        build_event("session.ended.v1", data)["id"] != build_event("session.ended.v1", data)["id"]
    )


def test_unknown_fields_are_rejected():
    with pytest.raises(ValidationError):
        build_event("session.ended.v1", {**_session_ended(), "transcript": "hola"})


def test_safety_flag_cannot_carry_text():
    flagged = json.loads((contracts_dir() / "examples/safety.flagged.v1.json").read_text())["data"]
    with pytest.raises(ValidationError):
        build_event(
            "safety.flagged.v1", {**flagged, "categories": ["quiero mover la olla caliente"]}
        )


def test_invalid_ids_are_rejected():
    with pytest.raises(ValidationError):
        build_event("session.ended.v1", {**_session_ended(), "session_id": "not-a-uuid"})
