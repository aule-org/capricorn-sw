// Draws a visual (contracts/visuals/visual.v1.schema.json) as SVG. It is a
// plain function from the description to a string, with no network, no image
// model and no state, so a drawing appears the moment the tutor asks for it.
// Black on white only: the same descriptions go to an e-ink screen.

export const TOPIC = "capricorn.visual";

const W = 640;
const H = 360;
const INK = "#111";
const FONT = "system-ui, -apple-system, 'Segoe UI', sans-serif";

const escape = (value) =>
  String(value).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);

// Numbers in a drawing are for reading: no long decimal tails.
const show = (n) => String(Math.round(n * 100) / 100);

// A character of this font is a little over half as wide as it is tall. That
// estimate is enough to shrink a long text until it fits the room it has;
// nothing is ever cut off.
const GLYPH = 0.58;
const fitted = (value, size, room) =>
  Math.round(Math.max(8, Math.min(size, room / (Math.max(String(value).length, 1) * GLYPH))) * 10) / 10;

const text = (x, y, value, { size = 18, anchor = "middle", weight = 400, room = W - 40 } = {}) =>
  `<text x="${x}" y="${y}" font-size="${fitted(value, size, room)}" font-weight="${weight}" text-anchor="${anchor}">${escape(value)}</text>`;

const line = (x1, y1, x2, y2, width = 2) =>
  `<line x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" stroke="${INK}" stroke-width="${width}"/>`;

const rect = (x, y, w, h, fill = "none") =>
  `<rect x="${x}" y="${y}" width="${w}" height="${h}" fill="${fill}" stroke="${INK}" stroke-width="2"/>`;

// A tight a/b token like 3/4 is a fraction to stack. A division written with
// spaces ("12 / 4") keeps its slash and stays plain text, so the two read apart.
const FRACTION = /(\d+)\/(\d+)/g;

// A line split into runs: plain text, and {num, den} fractions, left to right.
function runs(raw) {
  const parts = [];
  let last = 0;
  for (const match of raw.matchAll(FRACTION)) {
    if (match.index > last) parts.push({ text: raw.slice(last, match.index) });
    parts.push({ num: match[1], den: match[2] });
    last = match.index + match[0].length;
  }
  if (last < raw.length) parts.push({ text: raw.slice(last) });
  return parts.length ? parts : [{ text: raw }];
}

// Numerator and denominator are a little smaller than the surrounding text.
const DIGIT = 0.78;

// How wide a run is at a given size. A stacked fraction is as wide as its taller
// column of digits plus a little air; it is also taller than a line of text,
// which the caller leaves vertical room for.
function runWidth(run, size) {
  if (run.text !== undefined) return run.text.length * size * GLYPH;
  return Math.max(run.num.length, run.den.length) * size * DIGIT * GLYPH + size * 0.5;
}

// Draws a line of text with any a/b written as a stacked fraction, the whole
// line centred on x with its text baseline at y.
function inlineMath(x, y, raw, size) {
  const parts = runs(raw);
  let left = x - parts.reduce((sum, run) => sum + runWidth(run, size), 0) / 2;
  let out = "";
  for (const run of parts) {
    const w = runWidth(run, size);
    if (run.text !== undefined) {
      out += `<text x="${left}" y="${y}" font-size="${size}" font-weight="600" text-anchor="start">${escape(run.text)}</text>`;
    } else {
      const cx = left + w / 2;
      const small = Math.round(size * DIGIT * 10) / 10;
      const bar = y - size * 0.3;
      const half = (w - size * 0.5) / 2;
      out +=
        `<text x="${cx}" y="${bar - size * 0.12}" font-size="${small}" font-weight="600" text-anchor="middle">${escape(run.num)}</text>` +
        line(cx - half, bar, cx + half, bar, 2) +
        `<text x="${cx}" y="${bar + small * 0.95}" font-size="${small}" font-weight="600" text-anchor="middle">${escape(run.den)}</text>`;
    }
    left += w;
  }
  return out;
}

function exercise(v, top) {
  const lines = v.lines;
  const base = lines.length <= 2 ? 44 : lines.length <= 4 ? 32 : 26;
  // One size for every line, shrunk until the widest fits — stacked fractions
  // measured too — so the statement reads as one block and nothing is cut off.
  const widest = Math.max(
    ...lines.map((raw) => runs(raw).reduce((sum, run) => sum + runWidth(run, base), 0)),
    1,
  );
  const size = Math.round(Math.min(base, (base * (W - 40)) / widest) * 10) / 10;
  const gap = size * 1.6;
  const first = top + (H - top - gap * (lines.length - 1)) / 2;
  return lines.map((raw, i) => inlineMath(W / 2, first + i * gap, raw, size)).join("");
}

function fractionBars(v, top) {
  const left = 60;
  const width = 440;
  const height = Math.min(52, (H - top - 30) / v.fractions.length - 18);
  return v.fractions
    .map((fraction, i) => {
      const y = top + 10 + i * (height + 18);
      const parts = fraction.denominator;
      const part = width / parts;
      // More parts than the bar holds (an improper fraction) fill the whole bar.
      // Shaded parts are separated by white lines, so they can still be counted.
      const shaded = Math.min(fraction.numerator, parts);
      let cells = "";
      for (let k = 0; k < parts; k += 1) {
        const x = left + k * part;
        cells += `<rect x="${x}" y="${y}" width="${part}" height="${height}" fill="${k < shaded ? INK : "#fff"}"/>`;
      }
      for (let k = 1; k < parts; k += 1) {
        const x = left + k * part;
        cells += `<line x1="${x}" y1="${y}" x2="${x}" y2="${y + height}" stroke="${k < shaded ? "#fff" : INK}" stroke-width="2"/>`;
      }
      cells += rect(left, y, width, height);
      const label = fraction.label ?? `${fraction.numerator}/${fraction.denominator}`;
      return cells + text(left + width + 20, y + height / 2 + 8, label, { size: 24, anchor: "start", weight: 600, room: W - left - width - 30 });
    })
    .join("");
}

function numberLine(v, top) {
  const left = 50;
  const right = W - 50;
  const y = top + (H - top) * 0.62;
  const span = v.to - v.from;
  if (!(span > 0)) throw new Error("number_line: to must be greater than from");
  const x = (n) => left + ((n - v.from) / span) * (right - left);
  // Too many ticks to read: keep every k-th one.
  const count = Math.floor(span / v.step + 1e-9);
  const every = Math.ceil(count / 20);
  let out = line(left, y, right, y, 3);
  for (let i = 0; i <= count; i += every) {
    const n = v.from + i * v.step;
    out += line(x(n), y - 9, x(n), y + 9) + text(x(n), y + 32, show(n), { size: 16 });
  }
  for (const mark of v.marks ?? []) {
    if (mark < v.from || mark > v.to) continue;
    out += `<circle cx="${x(mark)}" cy="${y}" r="8" fill="${INK}"/>`;
  }
  for (const jump of v.jumps ?? []) {
    const a = x(Math.max(v.from, Math.min(v.to, jump.from)));
    const b = x(Math.max(v.from, Math.min(v.to, jump.to)));
    const lift = Math.min(70, 24 + Math.abs(b - a) / 4);
    out += `<path d="M ${a} ${y - 12} Q ${(a + b) / 2} ${y - 12 - lift * 2} ${b} ${y - 12}" fill="none" stroke="${INK}" stroke-width="2" marker-end="url(#arrow)"/>`;
    if (jump.label) out += text((a + b) / 2, y - 22 - lift, jump.label, { size: 18, weight: 600, room: 160 });
  }
  return out;
}

function dotArray(v, top) {
  const cell = Math.min((W - 80) / v.columns, (H - top - 20) / v.rows, 56);
  const left = (W - cell * v.columns) / 2;
  const first = top + (H - top - cell * v.rows) / 2;
  let out = "";
  for (let r = 0; r < v.rows; r += 1) {
    for (let c = 0; c < v.columns; c += 1) {
      out += `<circle cx="${left + c * cell + cell / 2}" cy="${first + r * cell + cell / 2}" r="${cell * 0.32}" fill="${INK}"/>`;
    }
  }
  return out;
}

function counters(v, top) {
  const labelled = v.groups.some((group) => group.label);
  const gap = 28; // blank space between one group and the next
  const area = { width: W - 60, height: H - top - (labelled ? 52 : 24) };
  // Each group is laid out as a near-square block of counters.
  const blocks = v.groups.map((group) => {
    const columns = Math.max(1, Math.round(Math.sqrt(group.count)));
    return { ...group, columns, rows: Math.ceil(group.count / columns) };
  });
  const columns = blocks.reduce((sum, block) => sum + block.columns, 0);
  const rows = Math.max(...blocks.map((block) => block.rows));
  const cell = Math.min((area.width - gap * (blocks.length - 1)) / columns, area.height / rows, 64);
  const radius = cell * 0.36;
  const width = blocks.reduce((sum, block) => sum + block.columns * cell, 0) + gap * (blocks.length - 1);
  const firstRow = top + (area.height - rows * cell) / 2 + 6;
  let left = (W - width) / 2;
  let out = "";
  for (const block of blocks) {
    for (let i = 0; i < block.count; i += 1) {
      const cx = left + (i % block.columns) * cell + cell / 2;
      const cy = firstRow + Math.floor(i / block.columns) * cell + cell / 2;
      // Open counters are an outline, solid ones are filled: the same drawing
      // tells two kinds apart on a black-on-white screen, without colour.
      out +=
        block.fill === "open"
          ? `<circle cx="${cx}" cy="${cy}" r="${radius}" fill="#fff" stroke="${INK}" stroke-width="2"/>`
          : `<circle cx="${cx}" cy="${cy}" r="${radius}" fill="${INK}"/>`;
    }
    if (block.label) {
      out += text(left + (block.columns * cell) / 2, firstRow + rows * cell + 26, block.label, {
        size: 18,
        weight: 600,
        room: block.columns * cell + gap,
      });
    }
    left += block.columns * cell + gap;
  }
  return out;
}

function table(v, top) {
  const columns = v.headers.length;
  const rows = [v.headers, ...v.cells.map((row) => Array.from({ length: columns }, (_, i) => row[i] ?? ""))];
  const width = Math.min(W - 60, columns * 170);
  const cell = width / columns;
  const height = Math.min(46, (H - top - 16) / rows.length);
  const left = (W - width) / 2;
  return rows
    .map((row, r) =>
      row
        .map(
          (value, c) =>
            rect(left + c * cell, top + 6 + r * height, cell, height) +
            text(left + c * cell + cell / 2, top + 6 + r * height + height / 2 + 6, value, {
              size: 18,
              weight: r === 0 ? 700 : 400,
              room: cell - 12,
            }),
        )
        .join(""),
    )
    .join("");
}

function barChart(v, top) {
  const base = H - 44;
  const left = 60;
  const slot = (W - left - 30) / v.bars.length;
  const tallest = Math.max(...v.bars.map((bar) => bar.value), 1);
  const scale = (base - top - 26) / tallest;
  return (
    line(left - 10, base, W - 20, base, 3) +
    v.bars
      .map((bar, i) => {
        const height = bar.value * scale;
        const x = left + i * slot + slot * 0.2;
        return (
          rect(x, base - height, slot * 0.6, height, INK) +
          text(x + slot * 0.3, base - height - 8, show(bar.value), { size: 16, weight: 600 }) +
          text(x + slot * 0.3, base + 24, bar.label, { size: 16, room: slot - 6 })
        );
      })
      .join("")
  );
}

function rectangle(v, top) {
  const room = { w: W - 220, h: H - top - 80 };
  const scale = Math.min(room.w / v.width, room.h / v.height);
  const w = Math.max(v.width * scale, 40);
  const h = Math.max(v.height * scale, 40);
  const x = (W - w) / 2;
  const y = top + (H - top - h) / 2 - 10;
  const unit = v.unit ? ` ${v.unit}` : "";
  return (
    rect(x, y, w, h) +
    text(x + w / 2, y + h + 30, `${show(v.width)}${unit}`, { size: 20, weight: 600 }) +
    text(x + w + 14, y + h / 2 + 7, `${show(v.height)}${unit}`, { size: 20, anchor: "start", weight: 600, room: W - x - w - 24 })
  );
}

const KINDS = {
  exercise,
  fraction_bars: fractionBars,
  number_line: numberLine,
  dot_array: dotArray,
  counters,
  table,
  bar_chart: barChart,
  rectangle,
};

/** The visual as an SVG document. Throws on a kind it does not know. */
export function renderVisual(visual) {
  const draw = KINDS[visual?.kind];
  if (!draw) throw new Error(`unknown kind of visual: ${visual?.kind}`);
  const top = visual.title ? 58 : 20;
  const title = visual.title ? text(W / 2, 38, visual.title, { size: 22, weight: 600 }) : "";
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${W} ${H}" role="img" aria-label="${escape(visual.title ?? visual.kind)}" font-family="${FONT}" fill="${INK}">` +
    `<defs><marker id="arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="8" markerHeight="8" orient="auto-start-reverse"><path d="M 0 0 L 10 5 L 0 10 z" fill="${INK}"/></marker></defs>` +
    `<rect width="${W}" height="${H}" fill="#fff"/>` +
    title +
    draw(visual, top) +
    "</svg>"
  );
}
