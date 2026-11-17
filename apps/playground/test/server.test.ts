import assert from "node:assert/strict";
import type { AddressInfo } from "node:net";
import { after, before, test } from "node:test";

import { TokenVerifier } from "livekit-server-sdk";

import { readSettings } from "../src/config.ts";
import { createPlayground } from "../src/server.ts";
import { COMPLETE } from "./fixtures.ts";

const settings = readSettings(COMPLETE);
const server = createPlayground(settings);
let base = "";

before(async () => {
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  base = `http://127.0.0.1:${(server.address() as AddressInfo).port}/playground`;
});
after(() => server.close());

const start = (body: unknown) =>
  fetch(`${base}/api/session`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: typeof body === "string" ? body : JSON.stringify(body),
  });

test("serves the page and its scripts with a strict policy", async () => {
  for (const path of ["/", "/app.js", "/style.css", "/livekit-client.js"]) {
    const response = await fetch(base + path);
    assert.equal(response.status, 200, path);
    assert.match(response.headers.get("content-security-policy") ?? "", /default-src 'none'/);
    assert.equal(response.headers.get("cache-control"), "no-store");
    await response.arrayBuffer();
  }
  const policy = (await fetch(`${base}/`)).headers.get("content-security-policy") ?? "";
  assert.match(policy, /connect-src 'self' wss:\/\/livekit\.example\.org https:\/\/livekit\.example\.org/);
});

test("redirects the bare path and hides everything outside it", async () => {
  const bare = await fetch(base, { redirect: "manual" });
  assert.equal(bare.status, 308);
  assert.equal(bare.headers.get("location"), "/playground/");

  assert.equal((await fetch(base.replace("/playground", "/"))).status, 404);
  assert.equal((await fetch(`${base}/../etc/passwd`)).status, 404);
  assert.equal((await fetch(`${base}/api/session`)).status, 405);
});

test("refuses a wrong or missing code", async () => {
  for (const body of [{ code: "nope" }, {}, { code: 42 }]) {
    const response = await start(body);
    assert.equal(response.status, 401);
    assert.deepEqual(await response.json(), { error: "wrong_code" });
  }
  assert.equal((await start("not json")).status, 400);
  assert.equal((await start(JSON.stringify({ code: "x".repeat(5_000) }))).status, 400);
});

test("the right code gets a token for one room that asks for the tutor as an operator", async () => {
  const response = await start({ code: COMPLETE.PLAYGROUND_ACCESS_CODE, language: "en" });
  assert.equal(response.status, 200);
  const session = await response.json();

  assert.equal(session.serverUrl, "wss://livekit.example.org");
  assert.match(session.room, /^playground-[\w-]{12}$/);
  assert.equal(session.expiresInSeconds, 1200);

  const verifier = new TokenVerifier(COMPLETE.LIVEKIT_API_KEY, COMPLETE.LIVEKIT_API_SECRET);
  const claims = await verifier.verify(session.token);
  assert.equal(claims.video?.room, session.room);
  assert.equal(claims.video?.roomJoin, true);
  assert.deepEqual(claims.video?.canPublishSources, ["microphone", "camera"]);
  assert.ok(!claims.video?.roomAdmin && !claims.video?.roomCreate && !claims.video?.roomList);

  const [dispatch] = claims.roomConfig?.agents ?? [];
  assert.equal(dispatch?.agentName, "capricorn-tutor");
  assert.deepEqual(JSON.parse(dispatch?.metadata ?? "{}"), { audience: "operator", language: "en" });
});

test("an unknown language falls back to Spanish", async () => {
  const response = await start({ code: COMPLETE.PLAYGROUND_ACCESS_CODE, language: "fr" });
  const claims = await new TokenVerifier(
    COMPLETE.LIVEKIT_API_KEY,
    COMPLETE.LIVEKIT_API_SECRET,
  ).verify((await response.json()).token);

  assert.equal(JSON.parse(claims.roomConfig?.agents?.[0]?.metadata ?? "{}").language, "es");
});

test("too many wrong codes lock the endpoint, even for the right one", async () => {
  const locked = createPlayground(settings);
  await new Promise<void>((resolve) => locked.listen(0, "127.0.0.1", resolve));
  const url = `http://127.0.0.1:${(locked.address() as AddressInfo).port}/playground/api/session`;
  const post = (code: string) =>
    fetch(url, { method: "POST", body: JSON.stringify({ code }) }).then((r) => r.status);
  try {
    for (let attempt = 0; attempt < 10; attempt += 1) assert.equal(await post("guess"), 401);
    assert.equal(await post("guess"), 429);
    assert.equal(await post(COMPLETE.PLAYGROUND_ACCESS_CODE), 429);
  } finally {
    locked.close();
  }
});
