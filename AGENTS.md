# AGENTS.md

Read this before anything else in the repository. It is written for coding
agents; people should start at [README.md](README.md).

Capricorn is an AI voice tutor for children. This repository is its server.
Children use it, so a change that weakens a safety gate or leaks a child's data
is worse than a change that breaks a feature.

## Map

| Path | What | Language | Read first |
|---|---|---|---|
| `apps/platform` | API, database, learner model, memory, supervisor, exercise checker | Rust | `apps/platform/README.md` |
| `apps/agent` | The voice tutor on LiveKit Agents, and the two safety gates | Python 3.13+, `uv` | `apps/agent/README.md` |
| `apps/playground` | Web page to talk to the tutor | Node 24, TypeScript, no build step | `apps/playground/README.md` |
| `contracts/` | Events and visuals as JSON Schema; the source of truth between apps | JSON Schema | `contracts/README.md` |
| `curriculum/` | Skills and their prerequisites | JSON | `curriculum/README.md` |

The design, and what is built and what is not, is in [ARCHITECTURE.md](ARCHITECTURE.md).
Pull request rules are in [CONTRIBUTING.md](CONTRIBUTING.md).

## Commands

Run the ones for the app you touched. CI runs all of them on every pull request.

```bash
# contracts, curriculum, playground
pnpm install --frozen-lockfile
pnpm --filter @capricorn/playground typecheck
pnpm test

# agent
cd apps/agent
uv sync --locked
uv run ruff check . && uv run ruff format --check .
uv run pytest

# platform (needs DATABASE_URL to a PostgreSQL 17 with pgvector)
cd apps/platform
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

## Rules that are not negotiable

- **The safety gates fail closed.** A late, failed or malformed answer from a gate
  is "unavailable", never "allow". Anything a child can say, type or read passes a
  gate: do not add an input or an output path around `Gates` in `apps/agent`.
- **A child's name, the tutor's persona and what a child says are never logged**,
  and never put in anything the agent stores. Logs carry counts and identifiers.
- **Type safety.** Python is fully annotated, TypeScript is strict, and Rust returns
  an error for bad input instead of panicking. Validate at the boundary and keep
  types narrow inside.
- **The tutor's prompt stays short.** Every sentence is paid on every turn. Change a
  sentence before adding one, in Spanish and English together.
- **A tool the model can call has a hand-written schema with no nullable
  argument.** The model's API rejects an enum on a nullable type, and unit tests do
  not see that. After changing a tool or how the model is called, make one real
  call to the model before merging.
- **Contracts first.** A change to an event or a visual starts in `contracts/`,
  with an example, and then in the apps that produce and read it.
- **No secrets, keys, hostnames or access codes in the repository**, in code,
  tests, docs or commit messages.

## How to work here

- One pull request per logical change. Title: `type(scope): what it does`
  (see CONTRIBUTING.md). Everything in the repository is written in English;
  text the child or the family reads exists in Spanish and English.
- Match the code around you: its comment density, naming and idiom. Comments say
  why, not what.
- Prefer the simple option with the fewest moving parts. Do not add a service, a
  dependency or a setting for something a constant or a function can do.
- A setting is read in one place (`config.py` in the agent, `config.rs` in the
  platform), documented in `.env.example`, and its default keeps current behaviour.
- State what you verified and how. "Tests pass" is one claim; "works with the real
  model, in a real session" is another, and the pull request says which was done.
- Do not turn off, skip or loosen a failing check to get a change through.

## Where things are

| To change | Look at |
|---|---|
| What the tutor says and how it teaches | `apps/agent/src/capricorn_agent/tutor.py` |
| Speech, model and voice providers | `apps/agent/src/capricorn_agent/providers.py`, `config.py` |
| What a child may hear or say | `moderation.py`, `physical_safety.py`, `safety.py`, `replies.py` |
| What the screen can draw | `contracts/visuals/visual.v1.schema.json`, `apps/playground/public/visuals.js` |
| Checking a child's answer | `apps/platform/src/exercise.rs`, called through the agent's `pose_exercise` and `check_answer` tools |
| The plan, guidance and review of a session | `apps/platform/src/supervisor.rs` |
| What is known about a child | `apps/platform/src/learning/`, `apps/platform/src/memory.rs` |
| Settings | `.env.example` |
