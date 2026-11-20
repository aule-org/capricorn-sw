import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { startBody } from "../public/mail.js";
import { TEXTS, pickLanguage, text } from "../public/texts.js";

test("the page's language is the first one the browser asks for that Aule has", () => {
  assert.equal(pickLanguage(["en-US", "es-PE"]), "en");
  assert.equal(pickLanguage(["fr-FR", "es-PE", "en"]), "es");
  assert.equal(pickLanguage(["fr-FR"]), "es");
  assert.equal(pickLanguage([]), "es");
  assert.equal(pickLanguage(undefined), "es");
});

test("every text exists in Spanish and in English, with nothing left empty", () => {
  assert.deepEqual(Object.keys(TEXTS.en).sort(), Object.keys(TEXTS.es).sort());
  for (const language of ["es", "en"]) {
    for (const [key, value] of Object.entries(TEXTS[language])) {
      assert.ok(typeof value === "string" && value.trim(), `${language}.${key}`);
    }
  }
  assert.notEqual(text("en", "mail.send"), text("es", "mail.send"));
});

test("every text the page marks is in the table", () => {
  const html = readFileSync(new URL("../public/index.html", import.meta.url), "utf8");
  const marked = [...html.matchAll(/data-t="([^"]+)"/g)].map((found) => found[1]);
  assert.ok(marked.length >= 8);
  for (const key of marked) assert.ok(key in TEXTS.es, key);
});

test("the page's Spanish is what the page says without the script", () => {
  const html = readFileSync(new URL("../public/index.html", import.meta.url), "utf8");
  for (const [, key, inner] of html.matchAll(/data-t="([^"]+)"[^>]*>([^<]*)</g)) {
    assert.equal(inner.trim(), TEXTS.es[key], key);
  }
});

test("a code is not promised: the note after asking says nothing of what was sent", () => {
  for (const language of ["es", "en"]) {
    const note = text(language, "mail.sent");
    assert.doesNotMatch(note, /te enviamos|te mandamos|we('ve| have)? sent|we emailed|we mailed/i, language);
  }
  assert.equal(
    text("es", "mail.sent"),
    "Si el correo es correcto, te llegará un código en un minuto. Si no llega, revisa la carpeta de spam o vuelve a intentarlo en unos minutos.",
  );
});

test("the language chosen goes with the request for the code", () => {
  const address = { value: " ana@example.org " };
  assert.deepEqual(startBody(address, "en"), { email: "ana@example.org", language: "en" });
  assert.deepEqual(startBody(address, "es"), { email: "ana@example.org", language: "es" });
});
