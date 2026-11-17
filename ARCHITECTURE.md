# Architecture

Capricorn's server is a small event-driven system: a few components that own their data and tell each other what happened through versioned events. This document is the plan the code follows. Most of it is not built yet; the [status](#status) section says what is.

## Components

```text
                                capricorn-sw
              ┌──────────────────────────────────────────────────┐
  family app  │ platform (Node)   families · consent · devices   │
 ───────────▶ │                   sessions · learning · notices  │
  webhooks    │                   inbox table · outbox table     │
 ───────────▶ │ worker (Node)     event consumers, scheduled jobs│
              └───────▲──────────────────────────┬───────────────┘
              events  │                          │ room tokens
              ┌───────┴─────────┐                ▼
              │ agent (Python)  │ ◀──────── LiveKit ◀──────── Capricorn device
              │ LiveKit Agents  │
              └─────────────────┘
```

| Component | Language | Owns |
|---|---|---|
| `apps/platform` | Node, TypeScript | The API, the database, incoming webhooks, the outbox |
| `apps/worker` | Node, TypeScript | Reactions to events and scheduled jobs, in a separate process |
| `apps/agent` | Python | The conversation: speech, the tutor, the safety gates |
| `contracts/` | JSON Schema | The events all of them exchange |

The platform is one deployable made of modules with clear boundaries (a modular monolith), not a set of microservices. A module reads and writes only its own tables and talks to the others through events or their public functions.

A store (checkout, subscriptions, invoices) is not part of this repository. It talks to the platform only through events such as `order.paid`, so you can run Capricorn without one.

## Events

- **Envelope:** [CloudEvents 1.0](https://cloudevents.io) in structured JSON mode: `id`, `source`, `type`, `time`, `subject`, `data`.
- **Contracts:** one JSON Schema per event type and version in [`contracts/`](contracts/README.md). Both languages validate against the same files.
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
| Platform and worker in Node | One language with the web side, and good tooling for APIs and webhooks |
| JSON Schema contracts | The only way two languages can share types without one depending on the other |
| Modular monolith | A small team can run one service well; boundaries stay in the code, where they are cheap to move |
| Postgres as queue and event log | One database to back up and monitor, and events commit with the data they describe |

## Status

- Contracts for three events, validated by tests in Node and in Python.
- The agent's event builder.
- A first tutor in the agent: speech to text, Claude and text to speech on LiveKit Agents, with the person's camera as an image on each turn.
- The place of the two safety gates in a turn, with their fail-closed rule, but no gate behind it. Until both exist the agent refuses every session that is not marked as an operator's (an adult on the team), so no child can reach the tutor.
- The playground: a page behind an access code that opens an operator session.
- A compose file for the agent and the playground.
- CI for both languages.

Not built yet: the platform, the worker, the two gates, the events a session should publish, the outbox and inbox, and LiveKit and the database in the compose file.
