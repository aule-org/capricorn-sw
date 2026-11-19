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

export class PlatformError extends Error {
  readonly status: number;
  constructor(message: string, status = 502) {
    super(message);
    this.status = status;
  }
}

// `token` is a signed-in guardian's; without one the call is the team's own.
async function platform(settings: Settings, path: string, body?: unknown, token?: string): Promise<unknown> {
  const response = await fetch(`${settings.platform!.url}${path}`, {
    method: body === undefined ? "GET" : "POST",
    headers: {
      Authorization: `Bearer ${token ?? settings.platform!.adminToken}`,
      ...(body === undefined ? {} : { "Content-Type": "application/json" }),
    },
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(10_000),
  });
  const answer = (await response.json().catch(() => ({}))) as { error?: { message?: string } };
  if (!response.ok) {
    throw new PlatformError(answer.error?.message ?? `the platform answered ${response.status}`, response.status);
  }
  return answer;
}

/** The Google client people sign in with, or null when the platform has none. */
export async function googleClient(settings: Settings): Promise<string | null> {
  try {
    const answer = (await platform(settings, "/v1/auth/google")) as { client_id?: unknown };
    return typeof answer.client_id === "string" ? answer.client_id : null;
  } catch (error) {
    if (error instanceof PlatformError) return null;
    throw error;
  }
}

export type SignedIn = { token: string; days: number };

/** Trades what Google gave the page for a session of the person it names. */
export async function signIn(settings: Settings, credential: string): Promise<SignedIn> {
  const answer = (await platform(settings, "/v1/auth/google", { credential })) as {
    token?: unknown;
    expires_in_days?: unknown;
  };
  if (typeof answer.token !== "string") throw new PlatformError("the platform sent no session");
  return { token: answer.token, days: typeof answer.expires_in_days === "number" ? answer.expires_in_days : 1 };
}

export function signOut(settings: Settings, token: string): Promise<unknown> {
  return platform(settings, "/v1/auth/signout", {}, token);
}

export type Account = { name: string; familyId: string; students: { id: string; name: string }[] };

/** Who is signed in and the students of their family. */
export async function account(settings: Settings, token: string): Promise<Account> {
  const answer = (await platform(settings, "/v1/me", undefined, token)) as {
    guardian?: { name?: unknown };
    family_id?: unknown;
    children?: { id?: unknown; nickname?: unknown }[];
  };
  if (typeof answer.family_id !== "string") throw new PlatformError("the platform sent no family");
  return {
    name: typeof answer.guardian?.name === "string" ? answer.guardian.name : "",
    familyId: answer.family_id,
    students: (answer.children ?? []).flatMap((child) =>
      typeof child.id === "string" && typeof child.nickname === "string"
        ? [{ id: child.id, name: child.nickname }]
        : [],
    ),
  };
}

// The terms a guardian agrees to on this page; recorded with each consent.
const POLICY_VERSION = "playground-2026-11";

/**
 * A new student in the signed-in guardian's family, with the consent they just
 * gave on the page: the voice is processed to answer and what matters is
 * remembered between sessions. Transcripts are not kept.
 */
export async function createStudent(
  settings: Settings,
  token: string,
  familyId: string,
  name: string,
  language: Language,
): Promise<{ id: string; name: string }> {
  const created = (await platform(
    settings,
    `/v1/families/${encodeURIComponent(familyId)}/children`,
    { nickname: name, language },
    token,
  )) as { id?: unknown; child?: { id?: unknown } };
  const id = typeof created.id === "string" ? created.id : created.child?.id;
  if (typeof id !== "string") throw new PlatformError("the platform sent no student");
  for (const [purpose, granted] of [["voice", true], ["memory", true], ["transcripts", false]] as const) {
    await platform(
      settings,
      `/v1/children/${encodeURIComponent(id)}/consents`,
      { purpose, granted, policy_version: POLICY_VERSION },
      token,
    );
  }
  return { id, name };
}

/** A session for one of the signed-in guardian's students. */
export async function createStudentSession(
  settings: Settings,
  token: string,
  studentId: string,
  language: Language,
): Promise<Session> {
  const body = { audience: "child", language, child_id: studentId };
  return (await platform(settings, "/v1/sessions", body, token)) as Session;
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
