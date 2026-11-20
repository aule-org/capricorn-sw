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
