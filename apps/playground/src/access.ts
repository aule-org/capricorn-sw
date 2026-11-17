// The access code and a limit on wrong guesses.

import { createHash, timingSafeEqual } from "node:crypto";

const digest = (value: string) => createHash("sha256").update(value).digest();

/** Compares in constant time, whatever the lengths. */
export function sameCode(given: string, expected: string): boolean {
  return timingSafeEqual(digest(given), digest(expected));
}

/**
 * Counts wrong codes across all clients in a sliding window. The page sits
 * behind a proxy and is used by a handful of people, so one shared counter is
 * enough to make guessing pointless without trusting forwarded addresses.
 */
export class GuessLimiter {
  private failures: number[] = [];
  private readonly max: number;
  private readonly windowMs: number;

  constructor(max = 10, windowMs = 60_000) {
    this.max = max;
    this.windowMs = windowMs;
  }

  blocked(now = Date.now()): boolean {
    this.failures = this.failures.filter((at) => now - at < this.windowMs);
    return this.failures.length >= this.max;
  }

  recordFailure(now = Date.now()): void {
    this.failures.push(now);
  }
}
