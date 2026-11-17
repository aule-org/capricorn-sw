import assert from "node:assert/strict";
import { test } from "node:test";

import { ConfigError, readSettings } from "../src/config.ts";
import { COMPLETE } from "./fixtures.ts";


test("reads a complete environment with defaults", () => {
  const settings = readSettings(COMPLETE);

  assert.equal(settings.basePath, "/playground");
  assert.equal(settings.host, "127.0.0.1");
  assert.equal(settings.port, 8090);
  assert.equal(settings.agentName, "capricorn-tutor");
  assert.equal(settings.sessionMinutes, 20);
});

test("the browser URL can differ from the one the agent uses", () => {
  const settings = readSettings({
    ...COMPLETE,
    LIVEKIT_URL: "ws://127.0.0.1:7880",
    CAPRICORN_LIVEKIT_PUBLIC_URL: "wss://public.example.org",
  });

  assert.equal(settings.livekitUrl, "wss://public.example.org");
});

test("reports every problem at once", () => {
  assert.throws(
    () => readSettings({ PLAYGROUND_ACCESS_CODE: "short", PLAYGROUND_BASE_PATH: "play ground" }),
    (error: unknown) => {
      assert.ok(error instanceof ConfigError);
      for (const expected of [
        "CAPRICORN_LIVEKIT_PUBLIC_URL",
        "LIVEKIT_API_KEY is not set",
        "LIVEKIT_API_SECRET is not set",
        "PLAYGROUND_ACCESS_CODE must have at least 16",
        "PLAYGROUND_BASE_PATH",
      ]) {
        assert.match(error.message, new RegExp(expected));
      }
      return true;
    },
  );
});

test("the page can be served at the root", () => {
  assert.equal(readSettings({ ...COMPLETE, PLAYGROUND_BASE_PATH: "/" }).basePath, "");
});
