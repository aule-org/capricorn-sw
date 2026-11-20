// The page's own files, fingerprinted. Each is read once and given a hash of
// what it holds; asked for as "name?v=<hash>" it may be kept for a year, and
// every reference to it in the page, the stylesheet, the modules and the
// manifest says that address. A reload then asks only for the page, which is
// checked against its ETag and is usually a 304. A deploy changes the hashes,
// so nothing stale is ever kept.

import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";

export type AssetSource = { file: string | URL; type: string };
export type Asset = { body: Buffer; type: string; hash: string };

// The files whose references are rewritten: the page's own text, never a
// package's bundle (those are versioned, but their contents are left alone).
const REWRITTEN = /^(text\/html|text\/css|application\/manifest\+json)/;
const OWN_SCRIPT = /^text\/javascript/;

export function fingerprint(body: Buffer): string {
  return createHash("sha256").update(body).digest("hex").slice(0, 12);
}

const escape = (name: string) => name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

// A reference is a file's name in quotes or in url(): "style.css",
// './students.js', url("wall.png"). Pages are never versioned.
export function rewrite(text: string, versions: ReadonlyMap<string, string>): string {
  let out = text;
  for (const [name, hash] of versions) {
    out = out.replace(new RegExp(`(["'(])(\\./)?(${escape(name)})(?=["')])`, "g"), `$1$2$3?v=${hash}`);
  }
  return out;
}

/**
 * Reads the files and settles their hashes. A file's hash depends on the
 * hashes it writes into its references, so they are worked out from the
 * leaves up, repeating until nothing changes. A file that cannot be read is
 * left out, and asking for it is a 404.
 */
export async function loadAssets(
  sources: Record<string, AssetSource>,
  own: ReadonlySet<string>,
): Promise<Map<string, Asset>> {
  const raw = new Map<string, { body: Buffer; type: string }>();
  await Promise.all(
    Object.entries(sources).map(async ([path, source]) => {
      try {
        raw.set(path, { body: await readFile(source.file), type: source.type });
      } catch {
        // Missing on disk: answered as not found.
      }
    }),
  );

  const isPage = (type: string) => type.startsWith("text/html");
  const rewritten = (path: string, type: string) => REWRITTEN.test(type) || (OWN_SCRIPT.test(type) && own.has(path));
  const versions = new Map<string, string>();
  const bodies = new Map<string, Buffer>();
  for (let round = 0; round < 10; round++) {
    let changed = false;
    for (const [path, { body, type }] of raw) {
      const next = rewritten(path, type) ? Buffer.from(rewrite(body.toString("utf8"), versions)) : body;
      bodies.set(path, next);
      if (isPage(type)) continue;
      const hash = fingerprint(next);
      const name = path.slice(1);
      if (versions.get(name) !== hash) {
        versions.set(name, hash);
        changed = true;
      }
    }
    if (!changed) break;
  }

  const assets = new Map<string, Asset>();
  for (const [path, { type }] of raw) {
    const body = bodies.get(path) ?? Buffer.alloc(0);
    assets.set(path, { body, type, hash: fingerprint(body) });
  }
  return assets;
}

/** What a fingerprinted address may be kept for, and what anything else gets. */
export function cacheFor(asset: Asset, version: string | null): string {
  return version === asset.hash ? "public, max-age=31536000, immutable" : "no-cache";
}

/**
 * Whether a request's If-None-Match names this ETag. The comparison is the
 * weak one HTTP asks for here: Cloudflare marks the tag weak (W/"…") when it
 * compresses the file on the way out, and that still means the same file.
 */
export function stillFresh(ifNoneMatch: string | undefined, etag: string): boolean {
  if (!ifNoneMatch) return false;
  const bare = (tag: string) => tag.trim().replace(/^W\//, "");
  return ifNoneMatch.split(",").some((tag) => tag.trim() === "*" || bare(tag) === bare(etag));
}
