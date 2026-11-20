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
