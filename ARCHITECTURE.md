# Architecture

Capricorn's server is a platform that knows each child's learning, a voice agent that holds the conversation, and versioned events between them and the outside. This document is the plan the code follows. Not all of it is built; the [status](#status) section says what is.

## Components

```text
                                capricorn-sw
              ┌───────────────────────────────────────────────────┐
  family app  │ platform (Rust)   families · consent · sessions   │
 ───────────▶ │   serve           learning · memory · supervisor  │
  playground  │   worker          reviews, retention, housekeeping│
 ───────────▶ │                   Postgres + pgvector             │
              └───────▲──────────────────────────┬────────────────┘
        plan, turns,  │                          │ room tokens
        guidance      │                          ▼
              ┌───────┴─────────┐
              │ agent (Python)  │ ◀──────── LiveKit ◀──────── Capricorn device
              │ LiveKit Agents  │
              └─────────────────┘
```

| Component | Language | Owns |
|---|---|---|
| `apps/platform` | Rust | The API, the database, the learner model, memory, the supervisor, and the worker process that runs jobs |
| `apps/agent` | Python | The conversation: speech, the tutor, the safety gates |
| `apps/playground` | Node, TypeScript | A page for the people running a server to talk to the tutor |
| `contracts/` | JSON Schema | The events they exchange |

The platform is one crate that runs as two processes, `serve` (the API) and `worker` (jobs). It is a modular monolith, not a set of microservices: a module reads and writes only its own tables and talks to the others through their public functions.

A store (checkout, subscriptions, invoices) is not part of this repository. It talks to the platform only through events such as `order.paid`, so you can run Capricorn without one.

## How a session thinks

Three things decide what happens in a session, and only one of them is in the conversation.

**The learner model** (`learning`) estimates, without a language model, the chance that a child answers a question on a skill correctly: a global ability, an offset per skill, a practice term, the difficulty of the question and forgetting over time. It is a pure function of the child's history of answers, so correcting an answer and reading again recomputes everything, and a guardian or teacher can override a skill directly. The same signal that moves the child moves the difficulty of the question, so difficulty calibrates itself as children answer. From this the model offers candidates for the next session: reviews that are due, skills in progress, and new skills whose prerequisites are mastered.

**Memory** (`memory`) is what the tutor knows about a child between sessions: short statements derived from a session, never the transcript, stored with an embedding in pgvector. The embedding model is any OpenAI-compatible endpoint; the compose file runs one locally, so a child's memories do not leave the server to be embedded. A memory that repeats an existing one is not stored twice.

**The supervisor** (`supervisor`) is a second language model that thinks about the child's learning so the tutor can spend its attention on the conversation. It never sits between the child and the tutor:

| When | What it does | Cost to the conversation |
|---|---|---|
| Before | Writes a brief from the learner model's candidates, the child's profile and their memories: what to work on, how to start, what to keep in mind | None: it runs while the person connects, and the tutor starts from the learner model's own choices if the brief is late |
| During | Reads each turn after it happened, with the memories closest to what the child just said, and may write guidance for the tutor's next turns | None: the agent reports turns in the background and never waits. Guidance arrives one turn late by design |
| After | Reviews the session into evidence for the learner model, memories, a note for the guardian and fixed-category flags | None: it is a job in the worker |

Everything the supervisor returns is validated and cut to size before it is used, a skill it invents is dropped from a plan, and the conversation reaches it as data, never as instructions.

## Data about a child

- A child has no account. `children` holds what the tutor may know: a nickname, year of birth, grade, language, interests and a guardian's notes.
- The profile and the plan reach the agent through the platform, with a token that works for one session. They are never put in room metadata, which every participant can read.
- `consent_records` is append-only, enforced by the database. Each purpose (`voice`, `transcripts`, `memory`) has its own answer, the current one is the latest row, and the rows survive the erasure of the child as proof of what was agreed.
- Without consent for `voice` no session is created. Without `memory` nothing is remembered. Without `transcripts` the turns of a session are deleted as soon as its review finishes; with it they are deleted after a fixed time of at most 24 hours.
- Taking consent back deletes the data it covered. Erasing a child deletes sessions, turns, answers, memories and reviews with it.

## Events

- **Envelope:** [CloudEvents 1.0](https://cloudevents.io) in structured JSON mode: `id`, `source`, `type`, `time`, `subject`, `data`.
- **Contracts:** one JSON Schema per event type and version in [`contracts/`](contracts/README.md). Every language validates against the same files.
- **Outbox:** a component that changes data and publishes an event writes both in one database transaction. A relay delivers the event afterwards, so nothing is lost if the process dies in between.
- **Inbox:** every incoming webhook is verified, stored with its ID and processed once. A repeated delivery is acknowledged and ignored.
- **Delivery is at least once.** Consumers record the event IDs they have handled and must be safe to run twice.
- **Transport:** Postgres. Jobs and event delivery use the same database as the data, so there is no broker to run. A broker is only worth adding when one Postgres stops being enough.
- **The agent publishes through the platform:** it posts events over authenticated HTTP with the event ID as the idempotency key, and keeps a local retry queue so a session's report survives a platform restart.

### Catalog

Defined today:

| Event | Produced by | Meaning |
|---|---|---|
| `device.claimed.v1` | platform | A family linked a device by proving it has it |
| `session.ended.v1` | agent | A tutoring session finished |
| `safety.flagged.v1` | agent | A safety gate stopped a turn (categories only) |

Planned: `family.registered`, `consent.granted`, `consent.revoked`, `device.online`, `device.offline`, `session.started`, `transcript.ready`, `skill.updated`, `entitlement.granted`, `entitlement.ended`.

## Safety and privacy requirements

These are requirements, not goals. Code that breaks one does not get merged.

1. **Two gates on every child turn, and both fail closed.**
   - Content moderation checks what the child said before the tutor answers and what the tutor is about to say before it is spoken.
   - A physical-safety check ([TypeSafe](https://typesafe.ai) Jev) decides whether the child is describing or planning something that needs a nearby adult: heat, electricity, sharp objects, chemicals, medicines, choking, falls.
   - If either gate cannot answer in time, the child hears a fixed safe reply. An unchecked answer is never spoken.
2. **A child's session does not start without both gates configured and working.**
3. **Events and logs never carry a child's words, audio, transcripts or model scores.** Safety flags carry fixed categories only.
4. **No training on children's data,** by us or by any provider. Only providers whose terms allow minors' data with zero retention are supported out of the box.
5. **Audio is not stored.** Transcripts are kept only with a parent's consent, for a short fixed time, and a parent can delete them.
6. **Devices prove who they are.** Each device has its own credentials, and a family links one with a claim code that proves possession. An identifier such as a MAC address is never enough.
7. **Tokens are scoped.** The agent and each device get credentials limited to their own session, not a shared secret that opens everything.
8. **The hardware microphone switch wins.** The server never asks a device to bypass it.

## Decisions

| Decision | Why |
|---|---|
| Agent in Python | LiveKit Agents is developed in Python first: fixes and plugins land there before the Node port, and it has far more production use |
| Platform and worker in Rust | The learner model was already written in Rust; one binary runs the API and the worker, and it stays small on a cheap server |
| A supervisor beside the tutor, not in front of it | A second model in the path of every turn would add its latency to every answer. Out of the path it can be slower and smarter, and a failure costs nothing |
| Embeddings from a local model by default | A child's memories are the most personal data here; embedding them should not require sending them to a third party |
| JSON Schema contracts | The only way two languages can share types without one depending on the other |
| Modular monolith | A small team can run one service well; boundaries stay in the code, where they are cheap to move |
| Postgres as queue and event log | One database to back up and monitor, and events commit with the data they describe |

## Status

Built:

- Contracts for three events, validated by tests in Node and in Python.
- The platform in Rust: families, children, append-only consent, sessions created at token issue with a per-session agent token, the learner model with calibrated difficulty, vector memory, the supervisor (before, during, after), a Postgres job queue, and the housekeeping that deletes transcripts on time and closes abandoned sessions.
- The agent: a first tutor on LiveKit Agents with the camera as an image on each turn, connected to the platform for the plan, the turn reports and the guidance.
- The place of the two safety gates in a turn, with their fail-closed rule, but no gate behind it. Until both exist the agent refuses every session that is not marked as an operator's (an adult on the team), so no child can reach the tutor.
- The playground, which can create its sessions through the platform and shows the supervisor's plan and guidance.
- A compose file for all of it, next to an existing LiveKit server.

Not built yet:

- The two safety gates.
- Sign-in for families. The operator API uses one admin token in its place.
- Devices: claim codes, per-device credentials, firmware updates.
- Notices to guardians. Reviews and their flags are stored and can be read, but nothing is sent.
- The outbox and inbox, and with them the events of the catalog: the platform does not publish or consume any yet.
- A curriculum. The skill catalog is empty until one is loaded; until then skills are named by the review.

Not validated:

- The learner model's parameters come from public datasets (ASSISTments 2009 and 2017), not from Capricorn's children. Its forgetting term showed no measurable effect there. `apps/platform/examples/skill_model_eval.rs` refits them from a log of answers.
- The supervisor has only run against scripted answers in tests. How good its plans, guidance and reviews are with real conversations has not been measured.
