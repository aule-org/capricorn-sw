import assert from "node:assert/strict";
import type { AddressInfo } from "node:net";
import { after, before, test } from "node:test";

import { fingerprint, rewrite, stillFresh } from "../src/assets.ts";
import { readSettings } from "../src/config.ts";
import { createPlayground } from "../src/server.ts";
import { COMPLETE } from "./fixtures.ts";

const server = createPlayground(readSettings(COMPLETE));
let base = "";

before(async () => {
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  base = `http://127.0.0.1:${(server.address() as AddressInfo).port}/playground`;
});
after(() => server.close());

const YEAR = "public, max-age=31536000, immutable";
const versionOf = (text: string, name: string) =>
  text.match(new RegExp(`${name.replace(/\./g, "\\.")}\\?v=([0-9a-f]{12})`))?.[1];

test("the page points at fingerprinted files, and those are kept for a year", async () => {
  const page = await fetch(`${base}/`);
  assert.equal(page.headers.get("cache-control"), "no-cache");
  const html = await page.text();

  const css = versionOf(html, "style.css");
  assert.ok(css, "the stylesheet is fingerprinted");
  const style = await fetch(`${base}/style.css?v=${css}`);
  assert.equal(style.status, 200);
  assert.equal(style.headers.get("cache-control"), YEAR);
  const text = await style.text();

  for (const name of ["wall.png", "motifs.png", "dancers.png", "manta.png", "frame.png", "fonts/serif.woff2"]) {
    const version = versionOf(text, name);
    assert.ok(version, `${name} is fingerprinted in the stylesheet`);
    const file = await fetch(`${base}/${name}?v=${version}`);
    assert.equal(file.headers.get("cache-control"), YEAR, name);
    assert.equal(fingerprint(Buffer.from(await file.arrayBuffer())), version, `${name}'s version is its hash`);
  }
  for (const name of ["livekit-client.js", "wobnimo.js", "icon.svg", "manifest.webmanifest"]) {
    assert.ok(versionOf(html, name), `${name} is fingerprinted in the page`);
  }
});

test("a module has one address wherever it is named, so it runs once", async () => {
  const html = await (await fetch(`${base}/`)).text();
  const signin = versionOf(html, "signin.js");
  const app = versionOf(html, "app.js");
  const students = versionOf(html, "students.js");
  assert.ok(signin && app && students);

  const appText = await (await fetch(`${base}/app.js?v=${app}`)).text();
  assert.match(appText, new RegExp(`from "\\./signin\\.js\\?v=${signin}"`));
  const visuals = versionOf(appText, "visuals.js");
  assert.ok(visuals, "app.js imports visuals.js at its fingerprint");
  const visualsFile = await fetch(`${base}/visuals.js?v=${visuals}`);
  assert.equal(visualsFile.headers.get("cache-control"), YEAR);
  await visualsFile.arrayBuffer();
  const signinText = await (await fetch(`${base}/signin.js?v=${signin}`)).text();
  assert.match(signinText, new RegExp(`from "\\./students\\.js\\?v=${students}"`));

  const manifest = await (await fetch(`${base}/manifest.webmanifest?v=${versionOf(html, "manifest.webmanifest")}`)).text();
  assert.ok(versionOf(manifest, "icon.svg"), "the manifest's icon is fingerprinted");
});

test("without the right version a file is checked every time, and a match is a 304", async () => {
  const plain = await fetch(`${base}/style.css`);
  assert.equal(plain.headers.get("cache-control"), "no-cache");
  const etag = plain.headers.get("etag");
  assert.ok(etag);
  await plain.arrayBuffer();

  // An old page asking for an old version gets today's file, not kept.
  const stale = await fetch(`${base}/style.css?v=000000000000`);
  assert.equal(stale.headers.get("cache-control"), "no-cache");
  await stale.arrayBuffer();

  const again = await fetch(`${base}/style.css`, { headers: { "If-None-Match": etag } });
  assert.equal(again.status, 304);
  assert.match(again.headers.get("content-security-policy") ?? "", /default-src 'none'/);
  assert.equal(await again.text(), "");

  const page = await fetch(`${base}/`);
  const pageTag = page.headers.get("etag");
  await page.arrayBuffer();
  assert.equal((await fetch(`${base}/`, { headers: { "If-None-Match": pageTag ?? "" } })).status, 304);
});

test("answers about people are never kept", async () => {
  const account = await fetch(`${base}/api/account`);
  assert.equal(account.headers.get("cache-control"), "no-store");
  await account.arrayBuffer();
  const missing = await fetch(`${base}/nothing.png`);
  assert.equal(missing.status, 404);
  await missing.arrayBuffer();
});

test("rewrite versions whole names in quotes and url(), and nothing else", () => {
  const versions = new Map([
    ["app.js", "aaa"],
    ["wall.png", "bbb"],
  ]);
  assert.equal(rewrite(`<script src="app.js"></script>`, versions), `<script src="app.js?v=aaa"></script>`);
  assert.equal(rewrite(`import { x } from "./app.js";`, versions), `import { x } from "./app.js?v=aaa";`);
  assert.equal(rewrite(`background: url("wall.png");`, versions), `background: url("wall.png?v=bbb");`);
  assert.equal(rewrite(`"myapp.js" 'app.json' "app.jsx"`, versions), `"myapp.js" 'app.json' "app.jsx"`);
});

test("an ETag still matches when Cloudflare has marked it weak, or among others", () => {
  assert.equal(stillFresh('"abc"', '"abc"'), true);
  assert.equal(stillFresh('W/"abc"', '"abc"'), true);
  assert.equal(stillFresh('"x", W/"abc"', '"abc"'), true);
  assert.equal(stillFresh("*", '"abc"'), true);
  assert.equal(stillFresh('"abd"', '"abc"'), false);
  assert.equal(stillFresh(undefined, '"abc"'), false);
});
