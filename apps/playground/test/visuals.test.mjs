import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { test } from "node:test";

import { renderVisual } from "../public/visuals.js";

const examples = new URL("../../../contracts/visuals/examples/", import.meta.url);
const example = (name) => JSON.parse(readFileSync(new URL(`${name}.json`, examples), "utf8"));
const count = (svg, tag) => svg.split(`<${tag} `).length - 1;

test("every example in the contract can be drawn", () => {
  const names = readdirSync(examples).map((file) => file.replace(".json", ""));
  assert.ok(names.length >= 7);
  for (const name of names) {
    const svg = renderVisual(example(name));
    assert.match(svg, /^<svg xmlns="http:\/\/www\.w3\.org\/2000\/svg" viewBox="0 0 640 360"/, name);
    assert.match(svg, /<\/svg>$/, name);
    assert.doesNotMatch(svg, /NaN|undefined|Infinity/, name);
  }
});

test("the drawing uses black on white only", () => {
  for (const name of ["fraction_bars", "fraction_circle", "bar_chart", "number_line", "counters"]) {
    const colours = new Set(renderVisual(example(name)).match(/#[0-9a-f]{3,6}\b/gi));
    assert.deepEqual([...colours].sort(), ["#111", "#fff"], name);
  }
});

test("a fraction bar has one part per denominator and shades the numerator", () => {
  const svg = renderVisual({ kind: "fraction_bars", fractions: [{ numerator: 3, denominator: 8 }] });

  assert.equal(count(svg, "rect") - 2, 8, "eight parts, plus the background and the outline");
  assert.equal(svg.match(/<rect [^>]*fill="#111"\/>/g).length, 3, "three shaded parts");
  assert.equal(count(svg, "line"), 7, "a divider between every two parts");
  assert.equal(svg.split('stroke="#fff"').length - 1, 2, "white dividers between shaded parts");
  assert.match(svg, />3\/8</);
});

test("a fraction circle cuts the denominator into slices and shades the numerator", () => {
  const svg = renderVisual({ kind: "fraction_circle", fractions: [{ numerator: 3, denominator: 4 }] });

  assert.equal(count(svg, "path") - 1, 4, "four slices, plus the arrow head's own path");
  assert.equal((svg.match(/<path [^>]*fill="#111" stroke="#111"/g) || []).length, 3, "three shaded slices");
  assert.ok(svg.includes('fill="#fff" stroke="#111"'), "an unshaded slice");
  assert.match(svg, />3\/4</);

  // A whole (denominator 1) is a single circle, filled when the numerator is 1.
  const whole = renderVisual({ kind: "fraction_circle", fractions: [{ numerator: 1, denominator: 1 }] });
  assert.equal(count(whole, "circle"), 1);
});

test("an array draws rows times columns dots", () => {
  assert.equal(count(renderVisual({ kind: "dot_array", rows: 3, columns: 4 }), "circle"), 12);
  assert.equal(count(renderVisual({ kind: "dot_array", rows: 12, columns: 12 }), "circle"), 144);
});

test("an exercise stacks a tight fraction and leaves a spaced division as text", () => {
  const stacked = renderVisual({ kind: "exercise", lines: ["3/4 + 1/4 = ?"] });
  assert.ok(!stacked.includes("3/4"), "no literal slash fraction");
  assert.ok(stacked.includes(">3<") && stacked.includes(">4<"), "numerator and denominator drawn apart");
  assert.ok(count(stacked, "line") >= 2, "a dividing bar per fraction");
  assert.ok(stacked.includes("> + <"), "the operators stay as text");

  const division = renderVisual({ kind: "exercise", lines: ["12 / 4 = ?"] });
  assert.ok(division.includes("12 / 4 = ?"), "a spaced division keeps its slash");
  assert.equal(count(division, "line"), 0, "and draws no fraction bar");
});

test("counters draw each group with its own fill and label", () => {
  const svg = renderVisual({
    kind: "counters",
    groups: [
      { count: 5, fill: "solid", label: "rojas" },
      { count: 1, fill: "open", label: "azul" },
    ],
  });

  assert.equal(count(svg, "circle"), 6, "one circle per counter");
  assert.equal((svg.match(/<circle[^>]*fill="#111"\/>/g) || []).length, 5, "five solid");
  assert.equal((svg.match(/<circle[^>]*stroke="#111"[^>]*\/>/g) || []).length, 1, "one open outline");
  assert.ok(svg.includes(">rojas<") && svg.includes(">azul<"), "each group is named");
});

test("a number line labels its ticks, marks points and draws jumps", () => {
  const svg = renderVisual(example("number_line"));

  for (const label of ["0", "10", "20", "+6"]) assert.ok(svg.includes(`>${label}<`), label);
  assert.equal(count(svg, "circle"), 1);
  assert.equal(count(svg, "path") - 1, 1, "one jump, plus the arrow head's own path");
});

test("a long number line thins its ticks, and a backwards one is refused", () => {
  const svg = renderVisual({ kind: "number_line", from: 0, to: 1000, step: 1 });
  assert.ok(count(svg, "text") <= 21);

  assert.throws(() => renderVisual({ kind: "number_line", from: 5, to: 5, step: 1 }), /greater than from/);
});

test("a table keeps its columns even when a row is short", () => {
  const svg = renderVisual({ kind: "table", headers: ["a", "b", "c"], cells: [["1"], ["2", "3", "4", "5"]] });

  assert.equal(count(svg, "rect") - 1, 9);
  assert.ok(!svg.includes(">5<"));
});

test("text from the tutor cannot become markup", () => {
  const svg = renderVisual({ kind: "exercise", title: '<script>alert("x")</script>', lines: ["a < b & c > d"] });

  assert.ok(!svg.includes("<script"));
  assert.ok(svg.includes("a &lt; b &amp; c &gt; d"));
  assert.ok(svg.includes('aria-label="&lt;script&gt;'));
});

test("an unknown kind is refused", () => {
  assert.throws(() => renderVisual({ kind: "video" }), /unknown kind/);
  assert.throws(() => renderVisual(null), /unknown kind/);
});

test("long text is made smaller until it fits, never cut", () => {
  const widest = (svg) =>
    Math.max(...[...svg.matchAll(/font-size="([\d.]+)"[^>]*>([^<]*)</g)].map(([, size, value]) => Number(size) * value.length * 0.58));
  const long = "Tengo una bolsa: 5 pelotas rojas y 1 pelota azul adentro.";
  const cases = [
    { kind: "exercise", title: "t".repeat(80), lines: [long, "Sacas una sin mirar."] },
    { kind: "table", headers: ["h".repeat(24), "b"], cells: [["c".repeat(24), "1"]] },
    { kind: "bar_chart", bars: Array.from({ length: 8 }, (_, i) => ({ label: "l".repeat(16), value: i })) },
  ];
  for (const visual of cases) assert.ok(widest(renderVisual(visual)) <= 640, visual.kind);

  // Lines of one exercise share a size, so the statement reads as one block.
  const sizes = [...renderVisual({ kind: "exercise", lines: [long, "Sacas una sin mirar."] }).matchAll(/font-size="([\d.]+)" font-weight="600"/g)].map((m) => m[1]);
  assert.equal(new Set(sizes).size, 1);
  // A short one keeps its large size.
  assert.match(renderVisual({ kind: "exercise", lines: ["3/4 + 1/4 = ?"] }), /font-size="44"/);
});
