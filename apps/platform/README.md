# Platform

What Capricorn knows and decides outside the conversation: children and what their guardians consented to, sessions, the learner model, memory and the supervisor. One Rust crate, two processes:

```bash
capricorn-platform serve     # the API
capricorn-platform worker    # session reviews, retention, housekeeping
capricorn-platform migrate   # only bring the database up to date
```

`serve` and `worker` migrate on start, so either is enough for an empty database. [ARCHITECTURE.md](../../ARCHITECTURE.md) explains how the pieces think together.

## Modules

| Module | What it does |
|---|---|
| `learning` | The skill model (`skill_model`, pure), the history it is replayed from, calibrated question difficulty, the candidates for the next session |
| `memory` | Statements about a child with pgvector embeddings; recall by meaning, or by date without an embedding model |
| `supervisor` | The brief before a session, guidance during it, the review after it |
| `families` | Families, guardians, children, append-only consent |
| `sessions` | Room tokens, the agent's API for one session, the review job |
| `jobs` | The Postgres job queue and the housekeeping |
| `llm`, `livekit`, `auth`, `config`, `error` | Claude with a forced structured answer, room tokens, the two kinds of caller, settings, the error shape |

## API

Operator routes take `Authorization: Bearer $CAPRICORN_ADMIN_TOKEN`. That one token stands in for family sign-in, which is not built yet, so keep the API on loopback.

| Route | What it does |
|---|---|
| `POST /v1/families` | A family and its first guardian |
| `POST /v1/families/{id}/children` | A child's profile |
| `GET`, `PUT`, `DELETE /v1/children/{id}` | Read, replace or erase a child and everything derived from them |
| `POST /v1/children/{id}/consents` | Grant or take back `voice`, `transcripts` or `memory` |
| `GET /v1/children/{id}/skills` | What the child knows now, and what the model would work on next |
| `POST /v1/children/{id}/attempts` | Answers reported by a guardian or imported |
| `POST /v1/children/{id}/corrections` | "They already know this": override a skill |
| `GET /v1/children/{id}/memories?query=` | What is remembered, closest to the query first |
| `DELETE /v1/children/{id}/memories/{memory_id}` | Forget one thing |
| `GET /v1/children/{id}/reviews` | Notes for the guardian from past sessions |
| `PUT`, `GET /v1/skills` | The skill catalog |
| `POST /v1/sessions` | A session and its room token |
| `GET /v1/sessions/{id}/supervisor` | The plan and guidance of one session |

The agent's routes take the token of one session, which the agent receives in its dispatch metadata:

| Route | What it does |
|---|---|
| `GET /v1/agent/session` | Marks the session as running; returns the student's profile and the plan as instructions |
| `POST /v1/agent/session/turns` | Reports turns; returns guidance written since the last call |
| `POST /v1/agent/session/end` | Closes the session and queues its review |

Errors are `{"error": {"code": "...", "message": "..."}}`.

## Settings

`.env.example` at the repository root lists them. Only `DATABASE_URL` and `CAPRICORN_ADMIN_TOKEN` are required: without LiveKit keys no session can be created, without `ANTHROPIC_API_KEY` plans come from the learner model alone and there is no guidance or review, and without `CAPRICORN_EMBEDDINGS_URL` memories are recalled by date.

The embedding model must return 1024 dimensions (`bge-m3` does). Another size needs a migration.

## Develop

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test
```

The tests in `tests/api.rs` run the API against a real Postgres with pgvector, each in its own temporary database, with scripted stand-ins for the language model and the embedding model.

To refit the learner model's parameters from a log of answers:

```bash
cargo run --release --example skill_model_eval -- answers.tsv
```
