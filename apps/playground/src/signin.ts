// Signing in to the playground with Google. The platform decides who the person
// is; this server only carries their session in a cookie the page cannot read.

import { createHash } from "node:crypto";
import type { IncomingMessage } from "node:http";

const COOKIE = "aule_guardian";
// A platform session token: 32 random bytes in hex.
const TOKEN = /^[0-9a-f]{64}$/;

/** The signed-in guardian's platform token, or null when nobody is signed in. */
export function guardianToken(request: IncomingMessage): string | null {
  for (const part of (request.headers.cookie ?? "").split(";")) {
    const [name, value] = part.trim().split("=");
    if (name === COOKIE && value && TOKEN.test(value)) return value;
  }
  return null;
}

/** The cookie that keeps someone signed in, readable only by this server. */
export function signedInCookie(token: string, basePath: string, days: number): string {
  const seconds = Math.max(1, Math.floor(days * 24 * 60 * 60));
  return `${COOKIE}=${token}; Max-Age=${seconds}; Path=${basePath}/; HttpOnly; Secure; SameSite=Strict`;
}

export function signedOutCookie(basePath: string): string {
  return `${COOKIE}=; Max-Age=0; Path=${basePath}/; HttpOnly; Secure; SameSite=Strict`;
}

/**
 * How many sessions one sign-in may start in a day. The page is open to anyone
 * with a Google account and every session costs speech and model time, so this
 * bounds what one account can spend. It is kept in memory: a restart forgets it.
 */
export class DailyLimit {
  readonly #perDay: number;
  readonly #now: () => number;
  readonly #used = new Map<string, { day: number; count: number }>();

  constructor(perDay: number, now: () => number = Date.now) {
    this.#perDay = perDay;
    this.#now = now;
  }

  /** Counts one session for this sign-in. False when the day's sessions are used up. */
  take(token: string): boolean {
    const day = Math.floor(this.#now() / 86_400_000);
    // The token itself is never kept, only what tells two sign-ins apart.
    const key = createHash("sha256").update(token).digest("base64url");
    const entry = this.#used.get(key);
    if (!entry || entry.day !== day) {
      for (const [other, value] of this.#used) if (value.day !== day) this.#used.delete(other);
      this.#used.set(key, { day, count: 1 });
      return this.#perDay >= 1;
    }
    if (entry.count >= this.#perDay) return false;
    entry.count += 1;
    return true;
  }
}
