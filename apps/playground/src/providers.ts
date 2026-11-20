// Signing in with GitHub, Meta (Facebook) or Apple. The page sends the person
// to the provider with a one-time state, kept in a cookie; the provider sends
// them back here with a code, which the platform trades for who they are.

import { randomBytes, timingSafeEqual } from "node:crypto";
import type { IncomingMessage } from "node:http";

export const CODE_PROVIDERS = ["github", "meta", "apple"] as const;
export type CodeProvider = (typeof CODE_PROVIDERS)[number];

const COOKIE = "aule_signin";
// Long enough to sign in at the provider, even for the first time.
const STATE_SECONDS = 600;
// The Graph API version the platform uses too.
const META_VERSION = "v21.0";

export function authorizeUrl(provider: CodeProvider, clientId: string, redirectUri: string, state: string): string {
  const [base, extra]: [string, Record<string, string>] =
    provider === "github"
      ? ["https://github.com/login/oauth/authorize", { scope: "read:user user:email", allow_signup: "true" }]
      : provider === "meta"
        ? [`https://www.facebook.com/${META_VERSION}/dialog/oauth`, { scope: "email,public_profile", response_type: "code" }]
        : // Apple sends the name and address only with a form posted back.
          ["https://appleid.apple.com/auth/authorize", { scope: "name email", response_type: "code", response_mode: "form_post" }];
  const url = new URL(base);
  for (const [name, value] of Object.entries({ client_id: clientId, redirect_uri: redirectUri, state, ...extra })) {
    url.searchParams.set(name, value);
  }
  return url.toString();
}

export function newState(): string {
  return randomBytes(24).toString("base64url");
}

// Apple answers with a form posted from its own site, so the cookie has to
// travel on a cross-site request. It holds nothing but the state.
export function stateCookie(provider: CodeProvider, state: string, basePath: string): string {
  return `${COOKIE}=${provider}.${state}; Max-Age=${STATE_SECONDS}; Path=${basePath}/api/signin/; HttpOnly; Secure; SameSite=None`;
}

export function clearedStateCookie(basePath: string): string {
  return `${COOKIE}=; Max-Age=0; Path=${basePath}/api/signin/; HttpOnly; Secure; SameSite=None`;
}

/** Whether the provider came back with the state this browser was sent with. */
export function stateMatches(request: IncomingMessage, provider: CodeProvider, given: string): boolean {
  if (!given) return false;
  for (const part of (request.headers.cookie ?? "").split(";")) {
    const [name, ...rest] = part.trim().split("=");
    if (name !== COOKIE) continue;
    const kept = Buffer.from(rest.join("="));
    const expected = Buffer.from(`${provider}.${given}`);
    return kept.length === expected.length && timingSafeEqual(kept, expected);
  }
  return false;
}

/** The name Apple posts the first time someone signs in, if they shared it. */
export function appleName(user: string | null): string | null {
  if (!user) return null;
  try {
    const name = (JSON.parse(user) as { name?: { firstName?: unknown; lastName?: unknown } }).name;
    const parts = [name?.firstName, name?.lastName].filter((part): part is string => typeof part === "string");
    const joined = parts.join(" ").trim();
    return joined && joined.length <= 120 ? joined : null;
  } catch {
    return null;
  }
}
