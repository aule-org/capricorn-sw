# Agent

Capricorn's voice tutor: a [LiveKit Agents](https://github.com/livekit/agents) worker in Python. It joins a room when asked by name, listens, and answers out loud. When the person's camera is on, the tutor also gets the latest frame with each turn, so it can look at a notebook.

## What is here

| Module | What it does |
|---|---|
| `worker` | The LiveKit worker: one tutor session per room it is dispatched to, with a time limit |
| `tutor` | The tutor's instructions, and where the gates and the camera plug into a turn |
| `safety` | The two safety gates of [ARCHITECTURE.md](../../ARCHITECTURE.md) and who a session is for |
| `camera` | The latest camera frame, held in memory and never stored |
| `providers` | Builds the speech and language models from the settings |
| `platform` | Gets the student's profile and the supervisor's plan, reports turns in the background, applies guidance |
| `config` | Reads and checks the environment |
| `events` | Wraps what the agent reports in the shared [contracts](../../contracts/README.md) |

## Who it talks to

No safety gate is implemented yet. A session is for a child unless the server that created the room marked it `{"audience": "operator"}` in the dispatch metadata, and the agent refuses a child's session while either gate is missing. Today that means it only talks to the people running the server, through the [playground](../playground/README.md).

Not built yet: the gates themselves, and publishing `session.ended` and `safety.flagged`.

## Settings

Environment variables; `.env.example` at the repository root lists them all.

| Variable | Meaning |
|---|---|
| `LIVEKIT_URL`, `LIVEKIT_API_KEY`, `LIVEKIT_API_SECRET` | The LiveKit server |
| `CAPRICORN_STT` | `assemblyai`, `deepgram` or `openai` |
| `CAPRICORN_TTS` | `cartesia`, `elevenlabs` or `openai` |
| `ANTHROPIC_API_KEY` | The language model is Claude |
| `ASSEMBLYAI_API_KEY`, `DEEPGRAM_API_KEY`, `CARTESIA_API_KEY`, `ELEVEN_API_KEY`, `OPENAI_API_KEY` | Only for the providers chosen above |
| `CAPRICORN_TTS_VOICE` | Optional voice id |
| `CAPRICORN_LLM_MODEL` | Default `claude-sonnet-5-5` |
| `CAPRICORN_LANGUAGE` | `es` (default) or `en`; a session can ask for the other |
| `CAPRICORN_SESSION_MAX_MINUTES` | Default 15 |
| `CAPRICORN_AGENT_NAME` | Default `capricorn-tutor` |
| `CAPRICORN_PLATFORM_URL` | Optional. Where the [platform](../platform/README.md) is; without it the tutor has no memory of the student |

The worker refuses to start if a setting is missing and says which. Before using a provider with children, check that its terms allow minors' data with zero retention.

## Develop

```bash
uv sync
uv run ruff check . && uv run ruff format --check .
uv run pytest
uv run python -m capricorn_agent.worker dev
```

The contracts are read from `contracts/` at the repository root, or from `CAPRICORN_CONTRACTS_DIR` when the agent runs outside the repository (for example, in a container).
