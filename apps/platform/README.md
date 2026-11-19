# Platform

What Capricorn knows and decides outside the conversation: children and what their guardians consented to, sessions, the learner model, memory and the supervisor. One Rust crate, two processes:

```bash
capricorn-platform serve     # the API
capricorn-platform worker    # session reviews, retention, housekeeping
capricorn-platform migrate   # only bring the database up to date
capricorn-platform import-skills curriculum/math-primaria.json
```

`serve` and `worker` migrate on start, so either is enough for an empty database. [ARCHITECTURE.md](../../ARCHITECTURE.md) explains how the pieces think together.

## Modules

| Module | What it does |
|---|---|
| `learning` | The skill model (`skill_model`, pure), the history it is replayed from, calibrated question difficulty, the candidates for the next session |
| `memory` | Statements about a child with pgvector embeddings; recall by meaning, or by date without an embedding model |
| `supervisor` | The brief before a session, guidance during it, the review after it |
| `families`, `signin` | Families, guardians, children, append-only consent; sign-in by emailed code |
| `devices` | Registration, claim codes, signed check-in and sessions, firmware |
| `events`, `notices`, `push` | The outbox and its delivery; what families are told; pushes through Firebase Cloud Messaging |
| `sessions` | Room tokens, the agent's API for one session, the review job |
| `jobs` | The Postgres job queue and the housekeeping |
| `llm`, `livekit`, `mail`, `auth`, `config`, `error` | Claude with a forced structured answer, room tokens, the two kinds of caller, settings, the error shape |

## API

Three kinds of caller use `Authorization: Bearer ...`:

- the **operator**, with `CAPRICORN_ADMIN_TOKEN`, for running the server;
- a **guardian**, with the token they get by signing in, who reaches only their own family;
- the **agent**, with the token of one session.

Devices do not use a bearer token: they sign with the key burned into them.

### Sign-in and account (guardian)

| Route | What it does |
|---|---|
| `POST /v1/auth/start` | Sends a six-digit code to an email address. Answers the same whether or not the address has an account |
| `POST /v1/auth/verify` | Trades the code for a token. A new address becomes a new family and must send `name` |
| `GET /v1/auth/google` | The Google client an app must ask credentials for. 503 when Google sign-in is not set up |
| `POST /v1/auth/google` | Trades a Google ID token (`credential`) for a token. Same account as the emailed code for that address; a new address becomes a new family, named from the Google account unless `name` is sent |
| `POST /v1/auth/signout` | Ends this session |
| `GET`, `PUT /v1/me` | The guardian, their children, unread notices; language and whether notices are emailed or pushed |
| `DELETE /v1/me` | Erases the family and everything derived from it. Needs the email typed again |
| `GET /v1/notices`, `POST /v1/notices/{id}/read` | What the family has been told |
| `PUT`, `DELETE /v1/push/tokens` | The app registers this phone for pushes each time it starts, and forgets it on sign-out. Body: `{"token", "platform"}` with `android`, `ios` or `web` |

### Children and learning (guardian for their family, or operator)

| Route | What it does |
|---|---|
| `POST /v1/families/{id}/children` | A child's profile. `grade` is `P1` to `P6` or `S1` to `S5` |
| `GET`, `PUT`, `DELETE /v1/children/{id}` | Read, replace or erase a child and everything derived from them. `tutor_name` (1 to 20 letters, default `Capri`), `tutor_persona` (`calido`, `jugueton`, `aventurero` or `tranquilo`) and `tutor_persona_note` (one line, up to 200 characters; empty removes it) say how the tutor presents itself, and keep their value when left out |
| `POST /v1/children/{id}/consents` | Grant or take back `voice`, `transcripts` or `memory` |
| `GET /v1/children/{id}/skills` | What the child knows now, and what the model would work on next |
| `POST /v1/children/{id}/attempts` | Answers reported by a guardian (the operator can also import past records) |
| `POST /v1/children/{id}/corrections` | "They already know this": override a skill |
| `GET /v1/children/{id}/memories?query=` | What is remembered, closest to the query first |
| `DELETE /v1/children/{id}/memories/{memory_id}` | Forget one thing |
| `GET /v1/children/{id}/reviews` | Notes from past sessions |
| `POST /v1/sessions` | A session and its room token. A guardian can only start a child's session |
| `GET /v1/skills` | The skill catalog |

### Devices

| Route | Caller | What it does |
|---|---|---|
| `POST /v1/devices` | operator | Registers devices as they are made: serial, HMAC key, model |
| `POST /v1/firmware` | operator | Publishes a firmware version for a model. A version is never replaced |
| `GET /v1/devices` | guardian, operator | The family's devices |
| `POST /v1/devices/claim` | guardian | Links the device that shows this code, optionally to a child |
| `PUT`, `DELETE /v1/devices/{id}` | guardian, operator | Choose the child who uses it; unlink it |
| `POST /ota/` | device | Check-in: the time, newer firmware if there is any, and the code to show while it is not linked |
| `POST /ota/activate` | device | Signs the challenge that came with the code. 200 once linked, 202 while waiting for a guardian |
| `POST /ota/session` | device | Starts a session for its child. Signed with a fresh timestamp; each signature works once |

`/ota/` and `/ota/activate` follow the check-in and activation protocol of xiaozhi-esp32. `/ota/session` is Capricorn's own: the body is `{"serial_number", "timestamp", "hmac"}`, where `hmac` is HMAC-SHA256 of `session:<serial>:<timestamp>` under the device's key, in hex.

### Operator only

| Route | What it does |
|---|---|
| `POST /v1/families` | A family and its first guardian, without sign-in |
| `PUT /v1/skills` | Loads skills into the catalog (`capricorn-platform import-skills <file>` does the same from a file) |
| `GET /v1/sessions/{id}/supervisor` | The plan and guidance of one session and, once it has been reviewed, `review`: the summary and next step written for the family |

### Agent

| Route | What it does |
|---|---|
| `GET /v1/agent/session` | Marks the session as running; returns the student's profile and the plan as `instructions`, and `tutor` (`name`, `persona`, `note`): how the tutor presents itself to this child. The same three travel in the dispatch metadata as `tutor_name`, `tutor_persona`, `tutor_persona_note` |
| `POST /v1/agent/session/turns` | Reports turns; returns guidance written since the last call |
| `POST /v1/agent/session/flags` | Reports that a safety gate stopped a turn: the gate, what it did, fixed categories |
| `POST /v1/agent/session/end` | Closes the session and queues its review |

Errors are `{"error": {"code": "...", "message": "..."}}`.

## Events

Events are written to an outbox in the same transaction as the change they describe and delivered by the worker, at least once. Their shape is fixed by [`contracts/`](../../contracts/README.md), and the tests check every event the platform writes against those schemas.

| Event | When |
|---|---|
| `device.claimed.v1` | A family linked a device |
| `session.ended.v1` | A child's session on a device ended |
| `safety.flagged.v1` | A safety gate stopped a turn in a child's session |
| `session.reviewed.v1` | The supervisor finished reviewing a child's session |

The platform consumes its own events to write notices: a summary after each reviewed session, and one short, fixed-text note per session when a gate that listens to the child answered with support or asked for an adult. A new notice is also emailed and announced with a push. The push carries fixed text and the notice's id, never the child's name or what happened, because a locked phone shows it to whoever holds it.

With `CAPRICORN_EVENTS_WEBHOOK_URL` set, each event is also posted there as a CloudEvent with `X-Capricorn-Signature: sha256=<HMAC of the body>`.

## Settings

`.env.example` at the repository root lists them. Only `DATABASE_URL` and `CAPRICORN_ADMIN_TOKEN` are required: without LiveKit keys no session can be created, without mail settings nobody can sign in and notices stay in the app, without a Firebase service account there are no pushes, without `ANTHROPIC_API_KEY` plans come from the learner model alone and there is no guidance or review, and without `CAPRICORN_EMBEDDINGS_URL` memories are recalled by date.

The embedding model must return 1024 dimensions (`bge-m3` does). Another size needs a migration.

## Develop

Use Rust 1.94 or newer (required by SQLx 0.9) and PostgreSQL 17 with pgvector.

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test
```

The tests in `tests/api.rs` run the API against a real Postgres with pgvector, each in its own temporary database, with scripted stand-ins for the language model and the embedding model.

To refit the learner model's parameters from a log of answers:

```bash
cargo run --release --example skill_model_eval -- answers.tsv
```
