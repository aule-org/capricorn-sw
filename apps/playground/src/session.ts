// A playground session: one private room, one person, and a request for the
// tutor to join it.

import { randomBytes } from "node:crypto";

import { AccessToken, RoomAgentDispatch, RoomConfiguration, TrackSource } from "livekit-server-sdk";

import type { Language, Settings } from "./config.ts";

export type Session = {
  serverUrl: string;
  token: string;
  room: string;
  agentName: string;
  expiresInSeconds: number;
};

export async function createSession(settings: Settings, language: Language): Promise<Session> {
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
