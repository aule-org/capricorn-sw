import assert from "node:assert/strict";
import { test } from "node:test";

import { GuessLimiter, sameCode } from "../src/access.ts";

test("only the exact code matches", () => {
  assert.ok(sameCode("correct horse battery", "correct horse battery"));
  assert.ok(!sameCode("correct horse batter", "correct horse battery"));
  assert.ok(!sameCode("", "correct horse battery"));
});

test("wrong guesses block everyone until the window passes", () => {
  const limiter = new GuessLimiter(3, 1_000);
  for (const at of [0, 100, 200]) {
    assert.ok(!limiter.blocked(at));
    limiter.recordFailure(at);
  }

  assert.ok(limiter.blocked(900));
  assert.ok(!limiter.blocked(1_250));
});
