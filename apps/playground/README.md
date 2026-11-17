# Playground

A web page to talk to the Capricorn tutor from a browser, with your microphone and your camera. It is for the people running a server: a way to hear the tutor and try changes before any device or child is involved.

It is a small Node server with no database. It serves the page and has one endpoint, which trades the access code for a LiveKit token. The token is good for one new room and asks the [agent](../agent/README.md) to join it as an operator session.

When the tutor shows something (an exercise, fraction bars, a number line...), the page draws it under the video from the description it receives; `public/visuals.js` is that drawing code, a plain function from the description to SVG.

## Settings

| Variable | Meaning |
|---|---|
| `LIVEKIT_API_KEY`, `LIVEKIT_API_SECRET` | Signs the room tokens |
| `CAPRICORN_LIVEKIT_PUBLIC_URL` | The LiveKit URL as the browser reaches it (falls back to `LIVEKIT_URL`) |
| `PLAYGROUND_ACCESS_CODE` | At least 16 characters. Whoever has it can start a session |
| `PLAYGROUND_HOST`, `PLAYGROUND_PORT` | Default `127.0.0.1:8090` |
| `PLAYGROUND_BASE_PATH` | Default `/playground` |
| `PLAYGROUND_SESSION_MINUTES` | How long a token lasts. Default 20 |
| `CAPRICORN_AGENT_NAME` | The agent to ask for. Default `capricorn-tutor` |
| `CAPRICORN_PLATFORM_URL`, `CAPRICORN_ADMIN_TOKEN` | Optional. With them, sessions are created by the [platform](../platform/README.md): the supervisor plans and follows them, and the page shows its plan and guidance |
| `PLAYGROUND_CHILD_ID` | Optional, with a platform: a child profile to try the tutor with. Its guardian must have consented to `voice` |

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
