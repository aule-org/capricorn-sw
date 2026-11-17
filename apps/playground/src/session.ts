// A playground session: one private room, one person, and a request for the
// tutor to join it.

import { randomBytes } from "node:crypto";

import { AccessToken, RoomAgentDispatch, RoomConfiguration, TrackSource } from "livekit-server-sdk";

import type { Language, Settings } from "./config.ts";

export type Session = {
  /** Set when the platform created the session. */
  session_id?: string;
  serverUrl: string;
  token: string;
  room: string;
  agentName: string;
  expiresInSeconds: number;
};

export class PlatformError extends Error {}

async function platform(settings: Settings, path: string, body?: unknown): Promise<unknown> {
  const response = await fetch(`${settings.platform!.url}${path}`, {
    method: body === undefined ? "GET" : "POST",
    headers: {
      Authorization: `Bearer ${settings.platform!.adminToken}`,
      ...(body === undefined ? {} : { "Content-Type": "application/json" }),
    },
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(10_000),
  });
  const answer = (await response.json().catch(() => ({}))) as { error?: { message?: string } };
  if (!response.ok) {
    throw new PlatformError(answer.error?.message ?? `the platform answered ${response.status}`);
  }
  return answer;
}

/** What the supervisor planned and advised in a session the platform created. */
export function supervisorView(settings: Settings, sessionId: string): Promise<unknown> {
  return platform(settings, `/v1/sessions/${encodeURIComponent(sessionId)}/supervisor`);
}

export async function createSession(settings: Settings, language: Language): Promise<Session> {
  if (settings.platform) {
    // The platform creates the session, so the supervisor plans and follows it.
    const body = { audience: "operator", language, child_id: settings.platform.childId ?? undefined };
    return (await platform(settings, "/v1/sessions", body)) as Session;
  }
  const id = randomBytes(9).toString("base64url");
  const room = `playground-${id}`;
  const expiresInSeconds = settings.sessionMinutes * 60;

  const token = new AccessToken(settings.livekitApiKey, settings.livekitApiSecret, {
    identity: `operator-${id}`,
    name: "Operator",
    ttl: expiresInSeconds,
  });
  token.addGrant({
    room,
    roomJoin: true,
    canPublish: true,
    canSubscribe: true,
    canPublishData: true,
    canPublishSources: [TrackSource.MICROPHONE, TrackSource.CAMERA],
  });
  // The room is created when the person joins, already asking for the tutor.
  // "operator" tells the agent this is an adult on the team: it refuses any
  // other session until the safety gates exist.
  token.roomConfig = new RoomConfiguration({
    emptyTimeout: 30,
    departureTimeout: 10,
    maxParticipants: 2,
    agents: [
      new RoomAgentDispatch({
        agentName: settings.agentName,
        metadata: JSON.stringify({ audience: "operator", language }),
      }),
    ],
  });

  return {
    serverUrl: settings.livekitUrl,
    token: await token.toJwt(),
    room,
    agentName: settings.agentName,
    expiresInSeconds,
  };
}
