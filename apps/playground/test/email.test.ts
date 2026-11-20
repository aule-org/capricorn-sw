import assert from "node:assert/strict";
import type { AddressInfo } from "node:net";
import { createServer } from "node:http";
import { after, before, test } from "node:test";

import { readSettings } from "../src/config.ts";
import { createPlayground } from "../src/server.ts";
import { COMPLETE } from "./fixtures.ts";

const TOKEN = "e1".repeat(32);

// Stands in for the platform: codes go out for any address; 123456 is right,
// and ana@example.org already has an account.
const asked: { path: string; body: Record<string, unknown> }[] = [];
const platform = createServer(async (request, response) => {
  const chunks: Buffer[] = [];
  for await (const chunk of request) chunks.push(chunk as Buffer);
  const text = Buffer.concat(chunks).toString("utf8");
  const body = text ? JSON.parse(text) : {};
  const path = request.url ?? "";
  const answer = (status: number, value?: unknown) => {
    response.writeHead(status, { "Content-Type": "application/json" });
    response.end(value === undefined ? "" : JSON.stringify(value));
  };
  if (path === "/v1/auth/providers") return answer(200, { google: null, github: null, meta: null, apple: null, email: true });
  if (path === "/v1/auth/start") {
    asked.push({ path, body });
    return answer(204);
  }
  if (path === "/v1/auth/verify") {
    asked.push({ path, body });
    if (body.code !== "123456") return answer(403, { error: { message: "that code is wrong or has expired" } });
    if (body.email !== "ana@example.org" && !body.name) {
      return answer(400, { error: { message: "name is needed to create the account" } });
    }
    return answer(200, { token: TOKEN, expires_in_days: 30 });
  }
  if (path === "/v1/me") return answer(200, { guardian: { name: "Ana" }, family_id: "f", children: [] });
  if (path === "/v1/me/plan") return answer(200, { plan: "trial", ends_at: null, used: 0, allowed: 1, left: 1, team: false });
  answer(404, { error: { message: "no such path" } });
});

let base = "";
let server: ReturnType<typeof createPlayground>;

before(async () => {
  await new Promise<void>((resolve) => platform.listen(0, "127.0.0.1", resolve));
  const url = `http://127.0.0.1:${(platform.address() as AddressInfo).port}`;
  server = createPlayground(readSettings({ ...COMPLETE, AULE_PLATFORM_URL: url, AULE_ADMIN_TOKEN: "admin" }));
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  base = `http://127.0.0.1:${(server.address() as AddressInfo).port}/playground`;
});
after(() => {
  server.close();
  platform.close();
});

const post = (path: string, body: unknown) =>
  fetch(`${base}/api/signin/email/${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });

test("the page is told that a code can be emailed", async () => {
  const account = await (await fetch(`${base}/api/account`)).json();
  assert.deepEqual(account.providers, ["email"]);
});

test("a code goes out only for something that looks like an address", async () => {
  asked.length = 0;
  assert.equal((await post("start", { email: "not an address" })).status, 400);
  const sent = await post("start", { email: " ana@example.org " });
  assert.deepEqual([sent.status, await sent.json()], [200, { sent: true }]);
  assert.deepEqual(asked, [{ path: "/v1/auth/start", body: { email: "ana@example.org", language: "es" } }]);
});

test("the right code signs the person in with the usual cookie; a wrong one does not", async () => {
  const wrong = await post("verify", { email: "ana@example.org", code: "000000" });
  assert.deepEqual([wrong.status, await wrong.json()], [403, { error: "wrong_code" }]);
  assert.equal((await post("verify", { email: "ana@example.org", code: "12345" })).status, 400);

  const right = await post("verify", { email: "ana@example.org", code: "123456" });
  assert.equal(right.status, 200);
  assert.match(right.headers.get("set-cookie") ?? "", new RegExp(`^aule_guardian=${TOKEN};`));
  assert.equal((await right.json()).signed_in, true);
});

test("a new address is asked for a name, and gets an account with it", async () => {
  const unnamed = await post("verify", { email: "luis@example.org", code: "123456" });
  assert.deepEqual([unnamed.status, await unnamed.json()], [400, { error: "name_needed" }]);
  const named = await post("verify", { email: "luis@example.org", code: "123456", name: " Luis " });
  assert.equal(named.status, 200);
  assert.equal(asked.at(-1)?.body.name, "Luis");
});
