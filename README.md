# Capricorn Server

<p>
  <a href="https://github.com/aule-org/capricorn-hw"><img src="https://img.shields.io/badge/Hardware-capricorn--hw-1f6feb.svg" alt="Capricorn hardware"></a>
  <a href="https://github.com/aule-org/capricorn-fw"><img src="https://img.shields.io/badge/Firmware-capricorn--fw-2da44e.svg" alt="Capricorn firmware"></a>
  <a href="https://github.com/aule-org/capricorn-sw"><img src="https://img.shields.io/badge/Server-Capricorn-d97706.svg" alt="Capricorn server"></a>
  <a href="https://github.com/aule-org/capricorn-app"><img src="https://img.shields.io/badge/App-capricorn--app-8250df.svg" alt="Capricorn app"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-Apache%202.0-blue.svg" alt="Apache 2.0"></a>
</p>

**Capricorn is the open-source AI tutor.** This repository is its central server: what every Capricorn device and the family app talk to.

A tutor for children should not stop working when a company changes its plans. So the server is open too: you can run it at home, at a school, or use the one Aule hosts.

## What it does

- **The voice tutor.** A [LiveKit Agents](https://github.com/livekit/agents) worker joins the device's room, listens, thinks and answers out loud. It keeps turns short and patient, asks before it explains, and does not hand out answers.
- **Devices.** Links a new device to its family, issues per-device credentials and room tokens, and serves firmware updates over the air, speaking the bootstrap protocol of [xiaozhi-esp32](https://github.com/78/xiaozhi-esp32) that the [firmware](https://github.com/aule-org/capricorn-fw) is built on.
- **Families.** The API behind the [app](https://github.com/aule-org/capricorn-app): setup, progress, invitations, consent, privacy settings and deleting a child's data.
- **A model of what each child knows.** An open learner model estimates, skill by skill, what a child has mastered and what they are forgetting, and picks what to work on next. It needs no language model and can be corrected by a parent or a teacher.
- **A supervisor.** A second model plans each session, follows it without slowing the conversation down, and afterwards turns it into progress, memories and a note for the family.
- **Memory.** The tutor remembers what matters about a child between sessions, with embeddings computed on your own server.

## Running it yourself

The goal is one `docker compose up` on a computer or a small server: this service, a LiveKit server and a database. You bring the keys for the speech and language models you choose, or point it at models you run locally.

## Privacy by design

These are requirements for the code that lands here:

- No training on children's data.
- Audio is processed to answer and not stored by default. Transcripts are kept only with a parent's consent, and can be deleted from the app.
- Only speech and language providers whose terms allow minors' data and offer zero retention are supported out of the box.
- A device whose microphone switch is off sends nothing: the switch is in hardware, and the server never asks to bypass it.

## Status

Early. The repository has the platform (children, consent, sessions, the learner model, memory and the supervisor), a first voice tutor and a [playground](apps/playground/README.md) to talk to it from a browser. The tutor only accepts sessions from the people running the server: the safety gates are not written yet, and it refuses a child's session without them. Family sign-in, devices and notices are not written yet. [ARCHITECTURE.md](ARCHITECTURE.md) describes the design, what is built and what has not been validated.

## Repository layout

```text
apps/
  platform/   the API and the worker: children, consent, sessions, learning, memory, supervisor (Rust)
  agent/      the voice tutor on LiveKit Agents (Python)
  playground/ a web page to talk to the tutor with a microphone and a camera (Node, TypeScript)
contracts/    the events they exchange, as JSON Schema
compose.yaml  everything above with its database, next to your LiveKit server
```

```bash
pnpm install && pnpm test          # contracts and playground
cd apps/agent && uv run pytest     # agent
cd apps/platform && cargo test     # platform (needs DATABASE_URL to a Postgres with pgvector)
```

To run it, copy `.env.example` to `.env`, fill it in, run `docker compose up -d --build` and, once, `docker compose exec embeddings ollama pull bge-m3`.

## How the pieces fit

```
capricorn-hw   the device: board, e-ink, audio, enclosure
capricorn-fw   firmware: xiaozhi-esp32 + a LiveKit (WebRTC) transport
capricorn-sw   central server: voice tutor, devices, family API     (this repository)
capricorn-app  the family app: set up the device, see progress, privacy controls
```

## License

Apache License 2.0. See [LICENSE](LICENSE).

---

Made in Lima, Peru by [Aule](https://meetaule.com) · admin@meetaule.com
