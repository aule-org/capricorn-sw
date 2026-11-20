import assert from "node:assert/strict";
import { test } from "node:test";

import { methods } from "../public/mail.js";

test("each way in is shown for its own availability", () => {
  assert.deepEqual(methods({ google_client_id: "client-1", mail: true }), { google: true, mail: true, any: true });
  assert.deepEqual(methods({ google_client_id: "client-1" }), { google: true, mail: false, any: true });
  // The mailed code does not wait on Google.
  assert.deepEqual(methods({ google_client_id: null, mail: true }), { google: false, mail: true, any: true });
  assert.deepEqual(methods({ google_client_id: null, mail: false }), { google: false, mail: false, any: false });
  assert.deepEqual(methods({}), { google: false, mail: false, any: false });
});

import { clearName, verifyBody } from "../public/mail.js";

// The page's fields, as far as these functions touch them.
const fields = (over = {}) => ({
  address: { value: " ana@example.org " },
  digits: { value: "123456" },
  name: { value: " Ana " },
  nameLine: { hidden: true },
  ...over,
});

test("the name is sent only when the page asked for it", () => {
  // Typed, then the field was hidden again: not sent.
  assert.deepEqual(verifyBody(fields(), "es"), { email: "ana@example.org", code: "123456", language: "es" });
  assert.deepEqual(verifyBody(fields({ nameLine: { hidden: false } }), "en"), {
    email: "ana@example.org",
    code: "123456",
    language: "en",
    name: "Ana",
  });
  // Asked but left empty: nothing to send.
  assert.ok(!("name" in verifyBody(fields({ nameLine: { hidden: false }, name: { value: "  " } }), "es")));
});

test("using another address forgets the name that was typed", () => {
  const form = fields({ nameLine: { hidden: false } });

  clearName(form);

  assert.equal(form.name.value, "");
  assert.equal(form.nameLine.hidden, true);
  assert.ok(!("name" in verifyBody(form, "es")));
});

import { createAttempts, enterWithCode } from "../public/mail.js";

test("an answer belongs to the attempt that asked: changing the address drops the earlier ones", () => {
  const attempts = createAttempts();
  const first = attempts.begin();
  assert.equal(attempts.isCurrent(first), true);

  attempts.change();

  assert.equal(attempts.isCurrent(first), false);
  const second = attempts.begin();
  assert.equal(attempts.isCurrent(second), true);
  assert.equal(attempts.isCurrent(first), false);
});

// A call that answers when the test says so, as a slow network would.
function later() {
  const slot = {};
  slot.promise = new Promise((resolve, reject) => Object.assign(slot, { resolve, reject }));
  return slot;
}

test("a slow answer for the address that was left does not open the name field for the new one", async () => {
  const form = fields();
  const attempts = createAttempts();
  const slow = later();
  const calls = [];
  const call = (path, body) => {
    calls.push(body.email);
    return slow.promise;
  };

  // Address A: the person presses Enter and the check is still going.
  const pending = enterWithCode({ fields: form, language: "es", attempts, call });
  // Then "use another address" and a code for B.
  attempts.change();
  clearName(form);
  form.address.value = "beto@example.org";
  // The platform now says A's account is new and needs a name.
  slow.reject(new Error("name_needed"));

  assert.deepEqual(await pending, { kind: "stale" });
  assert.equal(form.nameLine.hidden, true, "A's answer must not ask for a name on B's form");
  assert.deepEqual(calls, ["ana@example.org"]);
  // B's own attempt is sent with B and no name.
  const next = await enterWithCode({ fields: form, language: "es", attempts, call: async (path, body) => ({ sent: body }) });
  assert.equal(next.kind, "account");
  assert.deepEqual(next.account.sent, { email: "beto@example.org", code: "123456", language: "es" });
});

test("a code answered in time is told apart: signed in, name needed, refused or failed", async () => {
  const run = (call, form = fields()) => enterWithCode({ fields: form, language: "es", attempts: createAttempts(), call });
  const fails = (message) => async () => { throw new Error(message); };

  assert.deepEqual(await run(async () => ({ signed_in: true })), { kind: "account", account: { signed_in: true } });
  const form = fields();
  assert.deepEqual(await run(fails("name_needed"), form), { kind: "name_needed" });
  assert.deepEqual(await run(fails("code_refused")), { kind: "refused" });
  assert.deepEqual(await run(fails("boom")), { kind: "failed" });
});
