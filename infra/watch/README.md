# Watch

Two scripts that mail the team when Aule stops working, and again when it works again. They hold no secrets: each reads its mail credentials from a file on its own machine.

| Script | Runs on | What it checks | Mails through |
|---|---|---|---|
| `aule-watch.sh` | the server (`/etc/aule/aule-watch.sh`, root-owned, run by `aule-admin`'s cron every 5 minutes) | the platform, the session page, the voice agent, LiveKit, the database, the worker, embeddings, the public page, the shop, disk use under 85 % and a nightly backup younger than 26 hours | the platform's own SMTP settings in `/etc/aule/aule.env` |
| `aule-watch-outside.sh` | the home server (`~/bin/aule-watch-outside.sh`, its user's cron every 5 minutes) | the public session page, LiveKit and the shop, from outside: the case where the whole server or its network is down | Resend's HTTP API, with the key in `~/.config/aule/resend-key` (mode 600) |

A failure is mailed when it starts and then every six hours while it lasts; a recovery is mailed once. The mail says which check failed and nothing about any family or child. `--test` sends one test mail.

To install or update on the server:

```sh
scp infra/watch/aule-watch.sh aule-prod:/tmp/
ssh aule-prod 'sudo install -o root -g root -m 755 /tmp/aule-watch.sh /etc/aule/aule-watch.sh && rm /tmp/aule-watch.sh'
# cron line, in aule-admin's crontab:
# */5 * * * * /usr/bin/flock -n /home/aule-admin/.local/state/aule/watch.lock /etc/aule/aule-watch.sh 2>&1 | /usr/bin/logger -t aule-watch
```

Not watched yet: whether the home server pulled last night's backup (its pull user's files are not readable by the watch).
