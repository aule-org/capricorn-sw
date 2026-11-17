# Agent

Capricorn's voice tutor: a [LiveKit Agents](https://github.com/livekit/agents) worker in Python. It joins a device's room, listens, runs every turn through the safety gates and answers out loud.

Today this package only has the event builder (`capricorn_agent.events`), which wraps what the agent reports in the shared [contracts](../../contracts/README.md) and refuses to build an event that does not validate. The tutor itself comes next, starting from the voice agent of our earlier device.

```bash
uv sync
uv run ruff check . && uv run ruff format --check .
uv run pytest
```

The contracts are read from `contracts/` at the repository root, or from `CAPRICORN_CONTRACTS_DIR` when the agent runs outside the repository (for example, in a container).
