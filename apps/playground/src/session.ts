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
export async function platform(
  settings: Settings,
  path: string,
  body?: unknown,
  token?: string,
  method?: "DELETE" | "PUT",
): Promise<unknown> {
  const response = await fetch(`${settings.platform!.url}${path}`, {
    method: method ?? (body === undefined ? "GET" : "POST"),
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

/** Asks the platform to mail a sign-in code to an address. It answers the same whether or not the address has an account. */
export async function startCode(settings: Settings, email: string, language: Language): Promise<void> {
  await platform(settings, "/v1/auth/start", { email, language });
}

/**
 * Trades the code mailed to an address for a session. An address seen for the
 * first time also needs the guardian's name: without it the platform refuses
 * and the code stays good, so the page can ask for the name and send both again.
 */
export async function verifyCode(
  settings: Settings,
  email: string,
  code: string,
  name: string | undefined,
  language: Language,
): Promise<SignedIn> {
  const answer = (await platform(settings, "/v1/auth/verify", {
    email,
    code,
    language,
    ...(name ? { name } : {}),
  })) as { token?: unknown; expires_in_days?: unknown };
  if (typeof answer.token !== "string") throw new PlatformError("the platform sent no session");
  return { token: answer.token, days: typeof answer.expires_in_days === "number" ? answer.expires_in_days : 1 };
}

/**
 * Erases the signed-in guardian's family: the account, its students and what
 * was kept about them. The platform only does it when the address typed is the
 * one the guardian signed in with.
 */
export function eraseAccount(settings: Settings, token: string, email: string): Promise<unknown> {
  return platform(settings, "/v1/me", { confirm_email: email }, token, "DELETE");
}

export function signOut(settings: Settings, token: string): Promise<unknown> {
  return platform(settings, "/v1/auth/signout", {}, token);
}

export type Account = {
  name: string;
  familyId: string;
  students: { id: string; name: string }[];
  /** The subjects the platform keeps for a student; null when it keeps none. */
  subjects: string[] | null;
};

/** Who is signed in and the students of their family. */
export async function account(settings: Settings, token: string): Promise<Account> {
  const answer = (await platform(settings, "/v1/me", undefined, token)) as {
    guardian?: { name?: unknown };
    family_id?: unknown;
    children?: { id?: unknown; nickname?: unknown }[];
    subjects?: unknown;
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
    subjects: Array.isArray(answer.subjects)
      ? answer.subjects.filter((subject): subject is string => typeof subject === "string")
      : null,
  };
}

/** What the supervisor planned and advised in a session the platform created. */
/**
 * Where a session run by the engine stands: the stage, the route and the card
 * of the exercise. Never the answer. `token` is the guardian's when signed in.
 */
export function step(settings: Settings, sessionId: string, token?: string): Promise<unknown> {
  return platform(settings, `/v1/sessions/${encodeURIComponent(sessionId)}/step`, undefined, token);
}

/** An answer written on the page, checked by the platform like a spoken one. */
export function answer(
  settings: Settings,
  sessionId: string,
  written: { answer: string; explanation?: string },
  token?: string,
): Promise<unknown> {
  return platform(settings, `/v1/sessions/${encodeURIComponent(sessionId)}/answers`, written, token);
}

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
