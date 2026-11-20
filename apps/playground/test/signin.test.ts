import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import type { AddressInfo } from "node:net";
import { createServer, type IncomingMessage } from "node:http";
import { after, before, test } from "node:test";

import { readSettings } from "../src/config.ts";
import { createPlayground } from "../src/server.ts";
import { DailyLimit, guardianToken, signedInCookie } from "../src/signin.ts";
import { COMPLETE } from "./fixtures.ts";

const TOKEN = "ab".repeat(32);
const STUDENT = "01a1f5f4-ede7-7028-a524-9d8e2f36b92c";

// Stands in for the platform: one guardian, whose students grow as they are created.
const seen: { method: string; path: string; auth: string | undefined; body: unknown }[] = [];
const students: { id: string; nickname: string }[] = [];
let googleOn = true;
const platform = createServer(async (request, response) => {
  const chunks: Buffer[] = [];
  for await (const chunk of request) chunks.push(chunk as Buffer);
  const text = Buffer.concat(chunks).toString("utf8");
  const body = text ? JSON.parse(text) : null;
  const path = request.url ?? "";
  seen.push({ method: request.method ?? "", path, auth: request.headers.authorization, body });
  const answer = (status: number, value: unknown) => {
    response.writeHead(status, { "Content-Type": "application/json" });
    response.end(JSON.stringify(value));
  };
  if (path === "/v1/auth/google" && request.method === "GET") {
    return googleOn ? answer(200, { client_id: "client-1" }) : answer(404, { error: { message: "google is not set up" } });
  }
  if (path === "/v1/auth/start") {
    if (!String(body.email).includes("@")) return answer(400, { error: { message: "email is not an email address" } });
    if (body.email === "sin-correo@example.org") return answer(503, { error: { message: "sign-in needs mail to be configured" } });
    return answer(204, {});
  }
  if (path === "/v1/auth/verify") {
    if (body.code !== "123456") return answer(403, { error: { message: "that code is wrong or has expired" } });
    // An address seen for the first time needs the guardian's name.
    if (body.email === "nueva@example.org" && !body.name) {
      return answer(400, { error: { message: "name is needed to create the account" } });
    }
    return answer(200, { token: TOKEN, expires_in_days: 30, new_account: body.email === "nueva@example.org" });
  }
  if (path === "/v1/auth/google") {
    if (body.credential !== "good") return answer(403, { error: { message: "refused" } });
    return answer(200, { token: TOKEN, expires_in_days: 30, new_account: true });
  }
  if (request.headers.authorization !== `Bearer ${TOKEN}`) return answer(401, { error: { message: "who?" } });
  if (path === "/v1/me" && request.method === "GET") {
    return answer(200, { guardian: { name: "Ana" }, family_id: "fam-1", children: students });
  }
  if (path === "/v1/families/fam-1/children") {
    students.push({ id: STUDENT, nickname: body.nickname });
    return answer(201, { id: STUDENT, nickname: body.nickname });
  }
  if (path === `/v1/children/${STUDENT}/consents`) return answer(201, {});
  if (path === "/v1/sessions") return answer(201, { session_id: "s-1", serverUrl: "wss://x", token: "jwt", room: "r" });
  if (path === "/v1/auth/signout") return answer(204, {});
  if (path === `/v1/children/${STUDENT}` && request.method === "DELETE") {
    students.length = 0;
    return answer(204, {});
  }
  if (path === "/v1/me" && request.method === "DELETE") {
    if (body.confirm_email !== "ana@example.org") return answer(400, { error: { message: "not yours" } });
    return answer(204, {});
  }
  answer(404, { error: { message: "no such path" } });
});

let base = "";
let server: ReturnType<typeof createPlayground>;

before(async () => {
  await new Promise<void>((resolve) => platform.listen(0, "127.0.0.1", resolve));
  const url = `http://127.0.0.1:${(platform.address() as AddressInfo).port}`;
  server = createPlayground(
    readSettings({ ...COMPLETE, AULE_PLATFORM_URL: url, AULE_ADMIN_TOKEN: "admin", PLAYGROUND_DAILY_SESSIONS: "2" }),
  );
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  base = `http://127.0.0.1:${(server.address() as AddressInfo).port}/playground`;
});
after(() => {
  server.close();
  platform.close();
});

const post = (path: string, body: unknown, cookie?: string) =>
  fetch(`${base}/api/${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json", ...(cookie ? { Cookie: cookie } : {}) },
    body: JSON.stringify(body),
  });
const COOKIE = `aule_guardian=${TOKEN}`;

test("the page learns the Google client and that nobody is signed in", async () => {
  const response = await fetch(`${base}/api/account`);

  assert.deepEqual(await response.json(), { google_client_id: "client-1", mail: true, signed_in: false });
});

test("the mailed code is offered whether or not Google is set up", async () => {
  googleOn = false;
  try {
    const response = await fetch(`${base}/api/account`);

    assert.deepEqual(await response.json(), { google_client_id: null, mail: true, signed_in: false });
    // Signed in, the page still knows a way in exists: it has no use for the access code open.
    const inside = await (await fetch(`${base}/api/account`, { headers: { Cookie: COOKIE } })).json();
    assert.equal(inside.signed_in, true);
    assert.equal(inside.mail, true);
  } finally {
    googleOn = true;
  }
});

test("a Google credential the platform accepts signs the person in with a cookie only the server reads", async () => {
  const response = await post("signin", { credential: "good" });

  assert.equal(response.status, 200);
  const cookie = response.headers.get("set-cookie") ?? "";
  assert.match(cookie, new RegExp(`^aule_guardian=${TOKEN}; Max-Age=2592000; Path=/playground/; HttpOnly; Secure; SameSite=Strict$`));
  // The page is told who it is and never sees the session.
  const body = await response.json();
  assert.deepEqual(body, { google_client_id: "client-1", mail: true, signed_in: true, name: "Ana", students: [], subjects: null });
  assert.ok(!JSON.stringify(body).includes(TOKEN));
});

test("a credential the platform refuses signs nobody in", async () => {
  const response = await post("signin", { credential: "forged" });

  assert.equal(response.status, 403);
  assert.equal(response.headers.get("set-cookie"), null);
  assert.equal((await post("signin", {})).status, 400);
});

test("a student is created only signed in, with a name and the guardian's yes", async () => {
  assert.equal((await post("student", { name: "Liam", consent: true })).status, 401);
  assert.equal((await post("student", { name: "Liam" }, COOKIE)).status, 400);
  assert.equal((await post("student", { name: " ", consent: true }, COOKIE)).status, 400);
  seen.length = 0;

  const response = await post("student", { name: " Liam ", consent: true, language: "es" }, COOKIE);

  assert.equal(response.status, 201);
  assert.deepEqual(await response.json(), { id: STUDENT, name: "Liam" });
  assert.ok(seen.every((call) => call.auth === `Bearer ${TOKEN}`), "every call is the guardian's own");
  assert.deepEqual(
    seen.filter((call) => call.path.endsWith("/consents")).map((call) => call.body),
    [
      { purpose: "voice", granted: true, policy_version: "playground-2026-11" },
      { purpose: "memory", granted: false, policy_version: "playground-2026-11" },
      { purpose: "transcripts", granted: false, policy_version: "playground-2026-11" },
    ],
  );
});

test("a student with only a username and the yes starts with memory off and the other consents as they were", async () => {
  assert.equal((await post("student", { name: "tigre_azul" }, COOKIE)).status, 400, "not without the yes");
  seen.length = 0;

  const response = await post("student", { name: "tigre_azul", consent: true }, COOKIE);

  assert.equal(response.status, 201);
  assert.deepEqual(seen.find((call) => call.path.endsWith("/children"))?.body, {
    nickname: "tigre_azul",
    grade: null,
    language: "es",
    interests: [],
    tutor_persona: "calido",
    tutor_persona_note: "",
    relationship: "",
  });
  assert.deepEqual(
    seen.filter((call) => call.path.endsWith("/consents")).map((call) => call.body),
    [
      { purpose: "voice", granted: true, policy_version: "playground-2026-11" },
      { purpose: "memory", granted: false, policy_version: "playground-2026-11" },
      { purpose: "transcripts", granted: false, policy_version: "playground-2026-11" },
    ],
  );
});

test("a code asked for by mail answers the same for any address and tells a bad one apart", async () => {
  seen.length = 0;

  const sent = await post("signin/start", { email: " Ana@Example.org ", language: "en" });

  assert.equal(sent.status, 200);
  assert.deepEqual(await sent.json(), { sent: true });
  assert.deepEqual(seen.at(-1)?.body, { email: "Ana@Example.org", language: "en" });
  assert.equal((await post("signin/start", { email: "no-es-un-correo" })).status, 400);
  assert.equal((await post("signin/start", {})).status, 400);
  const unmailed = await post("signin/start", { email: "sin-correo@example.org" });
  assert.equal(unmailed.status, 503);
  assert.deepEqual(await unmailed.json(), { error: "mail_unavailable" });
});

test("the code from the mail signs the person in with the same cookie as Google", async () => {
  seen.length = 0;

  const response = await post("signin/verify", { email: "ana@example.org", code: " 123 456 " });

  assert.equal(response.status, 200);
  assert.match(response.headers.get("set-cookie") ?? "", new RegExp(`^aule_guardian=${TOKEN}; Max-Age=2592000; Path=/playground/; HttpOnly; Secure; SameSite=Strict$`));
  const body = await response.json();
  assert.equal(body.signed_in, true);
  assert.equal(body.name, "Ana");
  assert.ok(!JSON.stringify(body).includes(TOKEN));
  assert.deepEqual(seen.find((call) => call.path === "/v1/auth/verify")?.body, {
    email: "ana@example.org",
    code: "123456",
    language: "es",
  });
});

test("a wrong or malformed code signs nobody in", async () => {
  for (const code of ["654321", "12345", "abcdef", "", 123456]) {
    seen.length = 0;
    const response = await post("signin/verify", { email: "ana@example.org", code });

    assert.equal(response.status, 403, String(code));
    assert.deepEqual(await response.json(), { error: "code_refused" });
    assert.equal(response.headers.get("set-cookie"), null);
  }
  assert.equal((await post("signin/verify", { code: "123456" })).status, 400);
});

test("an address seen for the first time is asked for a name, and the same code then works", async () => {
  const first = await post("signin/verify", { email: "nueva@example.org", code: "123456" });

  assert.equal(first.status, 400);
  assert.deepEqual(await first.json(), { error: "name_needed" });
  assert.equal(first.headers.get("set-cookie"), null);
  assert.equal((await post("signin/verify", { email: "nueva@example.org", code: "654321" })).status, 403, "the code is checked first");
  assert.equal((await post("signin/verify", { email: "nueva@example.org", code: "123456", name: "x".repeat(121) })).status, 400);

  const second = await post("signin/verify", { email: "nueva@example.org", code: "123456", name: " Ana " });

  assert.equal(second.status, 200);
  assert.equal((await second.json()).signed_in, true);
  assert.deepEqual(seen.findLast((call) => call.path === "/v1/auth/verify")?.body, {
    email: "nueva@example.org",
    code: "123456",
    language: "es",
    name: "Ana",
  });
});

test("a signed-in session is a student's, asked for as the guardian, and bounded per day", async () => {
  seen.length = 0;
  assert.equal((await post("session", { student_id: "nope" }, COOKIE)).status, 400);

  const first = await post("session", { student_id: STUDENT, language: "en" }, COOKIE);

  assert.equal(first.status, 200);
  assert.equal((await first.json()).session_id, "s-1");
  assert.deepEqual(seen.at(-1), {
    method: "POST",
    path: "/v1/sessions",
    auth: `Bearer ${TOKEN}`,
    body: { audience: "child", language: "en", child_id: STUDENT },
  });
  assert.equal((await post("session", { student_id: STUDENT }, COOKIE)).status, 200);
  const third = await post("session", { student_id: STUDENT }, COOKIE);
  assert.equal(third.status, 429);
  assert.deepEqual(await third.json(), { error: "daily_limit" });
});

test("signing out ends the platform session and clears the cookie", async () => {
  seen.length = 0;

  const response = await post("signout", {}, COOKIE);

  assert.match(response.headers.get("set-cookie") ?? "", /^aule_guardian=; Max-Age=0;/);
  assert.equal(seen[0]?.path, "/v1/auth/signout");
});

test("only a well-formed session cookie counts as signed in", () => {
  const request = (cookie: string) => ({ headers: { cookie } }) as IncomingMessage;

  assert.equal(guardianToken(request(`theme=dark; aule_guardian=${TOKEN}`)), TOKEN);
  assert.equal(guardianToken(request("aule_guardian=short")), null);
  assert.equal(guardianToken(request("")), null);
  assert.match(signedInCookie(TOKEN, "", 1), /Path=\/; HttpOnly; Secure; SameSite=Strict$/);
});

test("the daily limit counts per sign-in and starts over the next day", () => {
  let now = 0;
  const limit = new DailyLimit(1, () => now);

  assert.equal(limit.take("a"), true);
  assert.equal(limit.take("a"), false);
  assert.equal(limit.take("b"), true);
  now = 86_400_000;
  assert.equal(limit.take("a"), true);
});

test("a student is removed only signed in, as the guardian", async () => {
  assert.equal((await post("student/remove", { student_id: STUDENT })).status, 401);
  assert.equal((await post("student/remove", { student_id: "nope" }, COOKIE)).status, 400);
  seen.length = 0;

  const response = await post("student/remove", { student_id: STUDENT }, COOKIE);

  assert.equal(response.status, 200);
  assert.deepEqual(seen.at(-1), {
    method: "DELETE",
    path: `/v1/children/${STUDENT}`,
    auth: `Bearer ${TOKEN}`,
    body: null,
  });
});

test("the account is erased only with the address typed again, and the cookie goes with it", async () => {
  assert.equal((await post("account/erase", { email: "ana@example.org" })).status, 401);
  assert.equal((await post("account/erase", {}, COOKIE)).status, 400);
  const wrong = await post("account/erase", { email: "otra@example.org" }, COOKIE);
  assert.equal(wrong.status, 400);
  assert.deepEqual(await wrong.json(), { error: "wrong_email" });
  assert.equal(wrong.headers.get("set-cookie"), null);

  const response = await post("account/erase", { email: " ana@example.org " }, COOKIE);

  assert.equal(response.status, 200);
  assert.deepEqual(await response.json(), { signed_in: false, erased: true });
  assert.match(response.headers.get("set-cookie") ?? "", /^aule_guardian=; Max-Age=0;/);
  assert.deepEqual(seen.at(-1)?.body, { confirm_email: "ana@example.org" });
});

test("every script the page's scripts import is served", async () => {
  const dir = new URL("../public/", import.meta.url);
  const imported = new Set<string>();
  for (const file of readdirSync(dir).filter((name) => name.endsWith(".js"))) {
    for (const found of readFileSync(new URL(file, dir), "utf8").matchAll(/from "\.\/([\w-]+\.js)"/g)) imported.add(found[1]);
  }
  assert.ok(imported.has("mail.js") && imported.has("texts.js"));
  for (const file of imported) {
    const response = await fetch(`${base}/${file}`);
    assert.equal(response.status, 200, file);
    assert.match(response.headers.get("content-type") ?? "", /javascript/, file);
  }
});
