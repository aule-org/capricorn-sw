// Which ways in the page offers, and what it sends when a mailed code is used.
// Kept apart from the page so each can be tried without a browser.

/**
 * Each way in is shown by its own availability: Google when the platform has a
 * client, the mailed code when the server offers it. Neither waits on the other.
 */
export function methods(account) {
  const google = Boolean(account?.google_client_id);
  const mail = account?.mail === true;
  return { google, mail, any: google || mail };
}

/** What is sent to sign in with a code. The name goes only when the page asked for it. */
export function verifyBody(fields, language) {
  const asked = !fields.nameLine.hidden;
  const name = fields.name.value.trim();
  return {
    email: fields.address.value.trim(),
    code: fields.digits.value,
    language,
    ...(asked && name ? { name } : {}),
  };
}

/** Forgets the name and stops asking for it: it belonged to the address that was left. */
export function clearName(fields) {
  fields.name.value = "";
  fields.nameLine.hidden = true;
}

/** What is sent to ask for a code: the address and the language the page is in. */
export function startBody(address, language) {
  return { email: address.value.trim(), language };
}

/**
 * Numbers the attempts to sign in, so an answer that arrives late can tell it no
 * longer belongs to what is on screen. Changing the address ends every earlier one.
 */
export function createAttempts() {
  let current = 0;
  return {
    begin: () => ++current,
    change: () => void ++current,
    isCurrent: (id) => id === current,
  };
}

/**
 * Sends the code and says what came of it. An answer from an attempt that was left
 * (another address was chosen meanwhile) is "stale": the page ignores it entirely.
 */
export async function enterWithCode({ fields, language, attempts, call }) {
  const mine = attempts.begin();
  try {
    const account = await call("api/signin/verify", verifyBody(fields, language));
    return attempts.isCurrent(mine) ? { kind: "account", account } : { kind: "stale" };
  } catch (error) {
    if (!attempts.isCurrent(mine)) return { kind: "stale" };
    if (error?.message === "name_needed") return { kind: "name_needed" };
    return { kind: error?.message === "code_refused" ? "refused" : "failed" };
  }
}
