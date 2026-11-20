# Server

What runs on aule-prod outside the containers, kept here so a new server can be set up from the repository. The live copies are in `/etc/aule/` (root-owned; the settings file `aule.env` holds the secrets and is not here).

| File | On the server | What it does |
|---|---|---|
| `aule-compose.sh` | `/etc/aule/aule-compose.sh` | `docker compose` for this repository with the server's settings: `sudo -u aule-admin /etc/aule/aule-compose.sh up -d --build <service>` |
| `aule-backup.sh` | `/etc/aule/aule-backup.sh` | Encrypted backup of the database, every six hours |
| `RESTORE.md` | `/etc/aule/RESTORE.md` | How to restore one |
| `backup-tests.sh` | (run from the Mac with the key) | Backup and restore tests |

The alerts are in `infra/watch`.

`aule-admin`'s crontab on the server:

```
SHELL=/bin/sh
PATH=/usr/local/bin:/usr/bin:/bin
25 */6 * * * umask 077; /usr/bin/flock -n /home/aule-admin/.local/state/aule/aule-backup.lock /etc/aule/aule-backup.sh >> /home/aule-admin/.local/state/aule/aule-backup.log 2>&1
*/5 * * * * /usr/bin/flock -n /home/aule-admin/.local/state/aule/watch.lock /etc/aule/aule-watch.sh 2>&1 | /usr/bin/logger -t aule-watch
17 * * * * /opt/aule/infra/scripts/prune-docker-cache.sh 2>&1 | /usr/bin/logger -t aule-docker-cache
```
