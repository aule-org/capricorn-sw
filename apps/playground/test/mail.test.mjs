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
