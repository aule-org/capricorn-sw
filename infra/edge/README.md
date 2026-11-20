# Edge

The two Cloudflare Workers in front of the server: `playground.meetaule.com` (the session page and its API) and `livekit.meetaule.com` (LiveKit's signalling, WebSocket included; audio and video go straight to the server on its UDP and TCP ports). Each forwards to the server's address, kept as the Worker's secret `ORIGIN_URL` so it is not in this public repository. The server only accepts these ports from Cloudflare's address ranges.

```sh
cd infra/edge
wrangler deploy -c wrangler.playground.jsonc
wrangler secret put ORIGIN_URL -c wrangler.playground.jsonc   # http://<server>:8090
wrangler deploy -c wrangler.livekit.jsonc
wrangler secret put ORIGIN_URL -c wrangler.livekit.jsonc      # http://<server>:7880
```

The shop and the brand site have their own Worker in the store's repository.
