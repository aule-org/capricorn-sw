import assert from "node:assert/strict";
import type { AddressInfo } from "node:net";
import { createServer } from "node:http";
import { after, before, test } from "node:test";

import { readSettings } from "../src/config.ts";
import { appleName, authorizeUrl } from "../src/providers.ts";
import { createPlayground } from "../src/server.ts";
import { COMPLETE } from "./fixtures.ts";

const TOKEN = "cd".repeat(32);
const PAGE = "https://page.example.org";
const CALLBACK = `${PAGE}/playground/api/signin/github/callback`;

// Stands in for the platform: GitHub and Apple are set up, Meta is not, and a
// code is good when it says so.
const asked: { path: string; body: Record<string, unknown> }[] = [];
const platform = createServer(async (request, response) => {
  const chunks: Buffer[] = [];
  for await (const chunk of request) chunks.push(chunk as Buffer);
  const text = Buffer.concat(chunks).toString("utf8");
  const body = text ? JSON.parse(text) : {};
  const path = request.url ?? "";
  const answer = (status: number, value: unknown) => {
    response.writeHead(status, { "Content-Type": "application/json" });
    response.end(JSON.stringify(value));
  };
  if (path === "/v1/auth/providers") {
    return answer(200, { google: "client-1", github: "gh-1", meta: null, apple: "com.example.signin" });
  }
  if (path === "/v1/auth/github" || path === "/v1/auth/apple") {
    asked.push({ path, body });
    if (body.code !== "good") return answer(403, { error: { message: "that sign-in could not be confirmed" } });
    return answer(200, { token: TOKEN, expires_in_days: 30 });
  }
  answer(404, { error: { message: "no such path" } });
});

let base = "";
let server: ReturnType<typeof createPlayground>;

before(async () => {
  await new Promise<void>((resolve) => platform.listen(0, "127.0.0.1", resolve));
  const url = `http://127.0.0.1:${(platform.address() as AddressInfo).port}`;
  server = createPlayground(
    readSettings({ ...COMPLETE, AULE_PLATFORM_URL: url, AULE_ADMIN_TOKEN: "admin", PLAYGROUND_PUBLIC_URL: PAGE }),
  );
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  base = `http://127.0.0.1:${(server.address() as AddressInfo).port}/playground`;
});
after(() => {
  server.close();
  platform.close();
});

const go = (path: string, init: RequestInit = {}) => fetch(`${base}/api/signin/${path}`, { redirect: "manual", ...init });

/** Starts a sign-in and returns the state the provider would send back, with its cookie. */
async function started(provider: string): Promise<{ state: string; cookie: string; location: URL }> {
  const response = await go(`${provider}/start`);
  assert.equal(response.status, 303);
  const location = new URL(response.headers.get("location") ?? "");
  const cookie = (response.headers.get("set-cookie") ?? "").split(";")[0]!;
  return { state: location.searchParams.get("state") ?? "", cookie, location };
}

test("the page is told which providers to show", async () => {
  const account = await (await fetch(`${base}/api/account`)).json();

  assert.deepEqual(account.providers, ["github", "apple"]);
});

test("starting sends the person to the provider with a state kept in a cookie", async () => {
  const response = await go("github/start");

  assert.equal(response.status, 303);
  const location = new URL(response.headers.get("location") ?? "");
  assert.equal(location.origin + location.pathname, "https://github.com/login/oauth/authorize");
  assert.equal(location.searchParams.get("client_id"), "gh-1");
  assert.equal(location.searchParams.get("redirect_uri"), CALLBACK);
  const state = location.searchParams.get("state") ?? "";
  assert.ok(state.length >= 32);
  assert.equal(
    response.headers.get("set-cookie"),
    `aule_signin=github.${state}; Max-Age=600; Path=/playground/api/signin/; HttpOnly; Secure; SameSite=None`,
  );
  // A provider that is not set up has nothing to start.
  assert.equal((await go("meta/start")).status, 404);
});

test("a code comes back to a session in a cookie only the server reads", async () => {
  const { state, cookie } = await started("github");
  asked.length = 0;

  const response = await go(`github/callback?code=good&state=${state}`, { headers: { Cookie: cookie } });

  assert.equal(response.status, 303);
  assert.equal(response.headers.get("location"), "/playground/");
  const cookies = response.headers.getSetCookie();
  assert.match(cookies[0]!, new RegExp(`^aule_guardian=${TOKEN}; Max-Age=2592000; Path=/playground/; HttpOnly`));
  assert.match(cookies[1]!, /^aule_signin=; Max-Age=0;/);
  assert.deepEqual(asked, [{ path: "/v1/auth/github", body: { code: "good", redirect_uri: CALLBACK } }]);
});

test("without the state this browser was sent with, nobody signs in and the platform is not asked", async () => {
  const { state, cookie } = await started("github");
  asked.length = 0;

  for (const [query, headers] of [
    [`code=good&state=${state}`, {}],
    [`code=good&state=${state}x`, { Cookie: cookie }],
    ["code=good", { Cookie: cookie }],
    [`code=good&state=${state}`, { Cookie: cookie.replace("github.", "apple.") }],
  ] as const) {
    const response = await go(`github/callback?${query}`, { headers });
    assert.equal(response.headers.get("location"), "/playground/?signin=failed", query);
    assert.ok(!response.headers.getSetCookie().some((one) => one.startsWith("aule_guardian=")));
  }
  assert.deepEqual(asked, []);
});

test("saying no at the provider, or a code the platform refuses, lands back on the page", async () => {
  const { state, cookie } = await started("github");
  const said = await go(`github/callback?error=access_denied&state=${state}`, { headers: { Cookie: cookie } });
  assert.equal(said.headers.get("location"), "/playground/?signin=cancelled");

  const refused = await go(`github/callback?code=used&state=${state}`, { headers: { Cookie: cookie } });
  assert.equal(refused.headers.get("location"), "/playground/?signin=failed");
  assert.ok(!refused.headers.getSetCookie().some((one) => one.startsWith("aule_guardian=")));
});

test("Apple posts its answer as a form, with the name the first time", async () => {
  const { state, cookie, location } = await started("apple");
  assert.equal(location.searchParams.get("response_mode"), "form_post");
  asked.length = 0;

  const form = new URLSearchParams({
    code: "good",
    state,
    user: JSON.stringify({ name: { firstName: "Ana", lastName: "Quispe" }, email: "ana@privaterelay.appleid.com" }),
  });
  const response = await go("apple/callback", {
    method: "POST",
    headers: { Cookie: cookie, "Content-Type": "application/x-www-form-urlencoded" },
    body: form.toString(),
  });

  assert.equal(response.headers.get("location"), "/playground/");
  assert.equal(asked[0]?.body.name, "Ana Quispe");
  // Only Apple answers with a form.
  assert.equal((await go("github/callback", { method: "POST", body: "code=good" })).status, 405);
});

test("Apple's name is read only from what it posts", () => {
  assert.equal(appleName(JSON.stringify({ name: { firstName: " Ana ", lastName: "" } })), "Ana");
  for (const user of [null, "", "not json", "{}", JSON.stringify({ name: { firstName: "x".repeat(130) } })]) {
    assert.equal(appleName(user), null, String(user));
  }
});

test("each provider is asked only for who the person is and their address", () => {
  const scopes = Object.fromEntries(
    (["github", "meta", "apple"] as const).map((provider) => [
      provider,
      new URL(authorizeUrl(provider, "id", CALLBACK, "s")).searchParams.get("scope"),
    ]),
  );
  assert.deepEqual(scopes, { github: "read:user user:email", meta: "email,public_profile", apple: "name email" });
});

test("without a public address the page offers no provider that comes back to it", async () => {
  const quiet = createPlayground(
    readSettings({ ...COMPLETE, AULE_PLATFORM_URL: "http://127.0.0.1:1", AULE_ADMIN_TOKEN: "admin" }),
  );
  await new Promise<void>((resolve) => quiet.listen(0, "127.0.0.1", resolve));
  const at = `http://127.0.0.1:${(quiet.address() as AddressInfo).port}/playground`;
  try {
    assert.equal((await fetch(`${at}/api/signin/github/start`, { redirect: "manual" })).status, 404);
  } finally {
    quiet.close();
  }
  assert.throws(() => readSettings({ ...COMPLETE, PLAYGROUND_PUBLIC_URL: "http://page.example.org" }), /PLAYGROUND_PUBLIC_URL/);
});
