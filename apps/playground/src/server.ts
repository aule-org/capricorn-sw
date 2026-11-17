// The playground's HTTP server: a static page and one endpoint that trades the
// access code for a room token. It keeps no state and stores nothing.

import { readFile } from "node:fs/promises";
import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { GuessLimiter, sameCode } from "./access.ts";
import { ConfigError, LANGUAGES, readSettings, type Language, type Settings } from "./config.ts";
import { PlatformError, createSession, supervisorView } from "./session.ts";

const publicDir = new URL("../public/", import.meta.url);
// The browser build that ships inside the livekit-client package.
const livekitClient = join(
  dirname(createRequire(import.meta.url).resolve("livekit-client")),
  "livekit-client.umd.js",
);

const STATIC: Record<string, { file: string | URL; type: string }> = {
  "/": { file: new URL("index.html", publicDir), type: "text/html; charset=utf-8" },
  "/app.js": { file: new URL("app.js", publicDir), type: "text/javascript; charset=utf-8" },
  "/visuals.js": { file: new URL("visuals.js", publicDir), type: "text/javascript; charset=utf-8" },
  "/style.css": { file: new URL("style.css", publicDir), type: "text/css; charset=utf-8" },
  "/livekit-client.js": { file: livekitClient, type: "text/javascript; charset=utf-8" },
};

const MAX_BODY_BYTES = 2_000;

function securityHeaders(settings: Settings): Record<string, string> {
  const livekit = new URL(settings.livekitUrl);
  const signalling = `${livekit.protocol}//${livekit.host}`;
  const http = signalling.replace(/^ws/, "http");
  return {
    "Content-Security-Policy": [
      "default-src 'none'",
      "script-src 'self'",
      "style-src 'self'",
      `connect-src 'self' ${signalling} ${http}`,
      "media-src 'self' blob: mediastream:",
      "img-src 'self' data:",
      "base-uri 'none'",
      "form-action 'none'",
      "frame-ancestors 'none'",
    ].join("; "),
    "Permissions-Policy": "camera=(self), microphone=(self)",
    "Referrer-Policy": "no-referrer",
    "X-Content-Type-Options": "nosniff",
    "X-Robots-Tag": "noindex, nofollow",
    "Cache-Control": "no-store",
  };
}

async function readJson(request: IncomingMessage): Promise<Record<string, unknown>> {
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of request) {
    size += (chunk as Buffer).length;
    if (size > MAX_BODY_BYTES) throw new Error("body too large");
    chunks.push(chunk as Buffer);
  }
  const body: unknown = JSON.parse(Buffer.concat(chunks).toString("utf8"));
  if (typeof body !== "object" || body === null || Array.isArray(body)) {
    throw new Error("not an object");
  }
  return body as Record<string, unknown>;
}

export function createPlayground(settings: Settings): Server {
  const headers = securityHeaders(settings);
  const limiter = new GuessLimiter();

  const send = (response: ServerResponse, status: number, type: string, body: string | Buffer) => {
    response.writeHead(status, { ...headers, "Content-Type": type });
    response.end(body);
  };
  const json = (response: ServerResponse, status: number, body: unknown) =>
    send(response, status, "application/json; charset=utf-8", JSON.stringify(body));

  async function session(path: string, request: IncomingMessage, response: ServerResponse) {
    if (limiter.blocked()) return json(response, 429, { error: "too_many_attempts" });
    let body: Record<string, unknown>;
    try {
      body = await readJson(request);
    } catch {
      return json(response, 400, { error: "bad_request" });
    }
    if (typeof body.code !== "string" || !sameCode(body.code, settings.accessCode)) {
      limiter.recordFailure();
      return json(response, 401, { error: "wrong_code" });
    }
    const language = LANGUAGES.find((known) => known === body.language) ?? ("es" as Language);
    // With a platform, the page may also ask what the supervisor is doing.
    if (path === "/api/supervisor") {
      if (!settings.platform || typeof body.session_id !== "string") {
        return json(response, 404, { error: "not_found" });
      }
      return json(response, 200, await supervisorView(settings, body.session_id));
    }
    return json(response, 200, await createSession(settings, language));
  }

  return createServer((request, response) => {
    // Doubled slashes are folded, so "/" as the base path and a probe of
    // "//healthz" mean the same thing.
    const pathname = new URL(request.url ?? "/", "http://playground").pathname.replace(/\/{2,}/g, "/");
    if (pathname === settings.basePath && settings.basePath) {
      response.writeHead(308, { ...headers, Location: `${settings.basePath}/` });
      return response.end();
    }
    if (!pathname.startsWith(`${settings.basePath}/`)) {
      return json(response, 404, { error: "not_found" });
    }
    const path = pathname.slice(settings.basePath.length);

    const handle = async () => {
      if (path === "/healthz" && request.method === "GET") return json(response, 200, { ok: true });
      if (path === "/api/session" || path === "/api/supervisor") {
        if (request.method !== "POST") return json(response, 405, { error: "method_not_allowed" });
        return session(path, request, response);
      }
      const asset = STATIC[path];
      if (asset && request.method === "GET") {
        return send(response, 200, asset.type, await readFile(asset.file));
      }
      return json(response, 404, { error: "not_found" });
    };
    handle().catch((error: unknown) => {
      if (error instanceof PlatformError && !response.headersSent) {
        return json(response, 502, { error: "platform", message: error.message });
      }
      console.error("request failed:", error instanceof Error ? error.message : error);
      if (!response.headersSent) json(response, 500, { error: "internal" });
      else response.end();
    });
  });
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  let settings: Settings;
  try {
    settings = readSettings(process.env);
  } catch (error) {
    if (error instanceof ConfigError) {
      console.error(`capricorn-playground: ${error.message}`);
      process.exit(1);
    }
    throw error;
  }
  const server = createPlayground(settings);
  server.listen(settings.port, settings.host, () => {
    console.log(`playground on http://${settings.host}:${settings.port}${settings.basePath}/`);
  });
  for (const signal of ["SIGINT", "SIGTERM"] as const) {
    process.on(signal, () => server.close(() => process.exit(0)));
  }
}
