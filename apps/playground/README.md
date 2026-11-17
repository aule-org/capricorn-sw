# Playground

A web page to talk to the Capricorn tutor from a browser, with your microphone and your camera. It is for the people running a server: a way to hear the tutor and try changes before any device or child is involved.

It is a small Node server with no database. It serves the page and has one endpoint, which trades the access code for a LiveKit token. The token is good for one new room and asks the [agent](../agent/README.md) to join it as an operator session.

## Settings

| Variable | Meaning |
|---|---|
| `LIVEKIT_API_KEY`, `LIVEKIT_API_SECRET` | Signs the room tokens |
| `PLAYGROUND_LIVEKIT_URL` | The LiveKit URL as the browser reaches it (falls back to `LIVEKIT_URL`) |
| `PLAYGROUND_ACCESS_CODE` | At least 16 characters. Whoever has it can start a session |
| `PLAYGROUND_HOST`, `PLAYGROUND_PORT` | Default `127.0.0.1:8090` |
| `PLAYGROUND_BASE_PATH` | Default `/playground` |
| `PLAYGROUND_SESSION_MINUTES` | How long a token lasts. Default 20 |
| `CAPRICORN_AGENT_NAME` | The agent to ask for. Default `capricorn-tutor` |

Put it behind a reverse proxy with HTTPS: browsers only give the microphone and the camera to secure pages.

## What it does not do

- It is not a way for children to use the tutor. The session it opens is marked as an operator's, which is the only kind the agent accepts while the safety gates are missing.
- It stores nothing: no accounts, no audio, no transcripts.
- Ten wrong codes in a minute lock the endpoint for everyone until the minute passes.

## Develop

Node 24 runs the TypeScript sources directly.

```bash
pnpm install
pnpm --filter @capricorn/playground typecheck
pnpm --filter @capricorn/playground test
node --env-file=../../.env src/server.ts
```
