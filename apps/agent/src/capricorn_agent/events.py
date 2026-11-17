"""Build and validate the events the agent publishes.

The schemas in ``contracts/`` are the source of truth shared with the platform.
An event that does not validate is a bug in the agent, so ``build_event`` raises
instead of sending it.
"""

import json
import os
import uuid
from datetime import UTC, datetime
from functools import cache
from pathlib import Path
from typing import Any

from jsonschema import Draft202012Validator, FormatChecker

SOURCE = "capricorn/agent"


def contracts_dir() -> Path:
    """``CAPRICORN_CONTRACTS_DIR``, or ``contracts/`` at the repository root."""
    configured = os.environ.get("CAPRICORN_CONTRACTS_DIR")
    return Path(configured) if configured else Path(__file__).resolve().parents[4] / "contracts"


@cache
def _validator(relative_path: str) -> Draft202012Validator:
    schema = json.loads((contracts_dir() / relative_path).read_text(encoding="utf-8"))
    return Draft202012Validator(schema, format_checker=FormatChecker())


def validate_event(event: dict[str, Any]) -> None:
    """Raise ``jsonschema.ValidationError`` unless the envelope and its data are valid."""
    _validator("envelope.schema.json").validate(event)
    _validator(f"events/{event['type']}.schema.json").validate(event["data"])


def build_event(
    event_type: str, data: dict[str, Any], *, subject: str | None = None
) -> dict[str, Any]:
    """Wrap ``data`` in a validated CloudEvents envelope."""
    event: dict[str, Any] = {
        "specversion": "1.0",
        "id": str(uuid.uuid4()),
        "source": SOURCE,
        "type": event_type,
        "time": datetime.now(UTC).isoformat().replace("+00:00", "Z"),
        "datacontenttype": "application/json",
        "data": data,
    }
    if subject is not None:
        event["subject"] = subject
    validate_event(event)
    return event
