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
