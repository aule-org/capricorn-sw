// livekit.meetaule.com: LiveKit's signalling on the server, WebSocket included
// (a Request built from the incoming one carries the upgrade). The audio and
// video go straight to the server, not through here. ORIGIN_URL is a secret.
export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const origin = new URL(env.ORIGIN_URL);
    origin.pathname = url.pathname;
    origin.search = url.search;
    return fetch(new Request(origin.toString(), request));
  },
};
