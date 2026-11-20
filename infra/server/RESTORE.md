# Restoring Aule's database

Backups are made every six hours by `/etc/aule/aule-backup.sh` and kept in `/var/lib/aule/backups/aule-sw/YYYY-MM-DD/` on the server (14 days) and in `~aule-agent/respaldos/aule-sw/` on the home server (180 days, pulled every night at 04:40). Each file is a gzipped SQL dump encrypted to the key "Aule backups (aule-prod)", fingerprint F83F 6EAA 4847 DD58 18EA 5C8D 90DB CEBC A27B DDC5.

The private key is NOT on either server. It is on Gonzalo's Mac: `~/.config/aule/aule-backup-private-key.asc`, and the keyring in `~/.config/aule/backup-gpg`. Without it the backups cannot be read.

## Restore

A backup can be older than the code, and restoring it over newer tables fails (they depend on what it drops). So the schema is emptied first, and the platform brings the restored data up to date by itself when it starts.

1. Check the file: `sha256sum -c aule-STAMP.sql.gz.gpg.sha256`.
2. Stop what writes to the database:
   `ssh aule-prod 'sudo -u aule-admin /etc/aule/aule-compose.sh stop platform worker agent'`
3. Empty the schema:
   `ssh aule-prod "sudo -u aule-admin /etc/aule/aule-compose.sh exec -T postgres psql -U aule -d aule -c 'drop schema public cascade; create schema public;'"`
4. From the Mac, restore (the decrypted dump only goes through pipes):

       ssh aule-prod 'sudo -u aule-admin cat /var/lib/aule/backups/aule-sw/DAY/aule-STAMP.sql.gz.gpg' \
         | gpg --homedir ~/.config/aule/backup-gpg --batch --decrypt \
         | gunzip \
         | ssh aule-prod 'sudo -u aule-admin /etc/aule/aule-compose.sh exec -T postgres psql -v ON_ERROR_STOP=1 -U aule -d aule'

5. Start them again; the platform applies the migrations newer than the backup:
   `ssh aule-prod 'sudo -u aule-admin /etc/aule/aule-compose.sh up -d platform worker agent'`

### Backups from before the baseline

On 2026-11-20 migrations 0001 to 0016 were collapsed into `0017_baseline.sql`. A backup taken before that records migrations 1 to 16. Restored as above, the platform sees all sixteen, marks the backup as having run the baseline instead (its schema is the same, nothing in it changes), and migrates on from there.

A backup older than migration 16 (some of 1 to 16 missing) is refused at start with an error that says so. Migration 16 went live on 2026-11-20 at 18:05 UTC, so **every backup taken before 2026-11-20 19:02 UTC is in this case**. Restore it in two steps: run the platform image tagged `pre-baseline` (git tag `pre-baseline`) once against it, which brings it to 16, take a backup of that, and restore that backup with the current image. Such backups stop existing once the oldest backup kept (180 days at home) is from after the cutover.

## Test

`infra/server/backup-tests.sh`, run from the Mac, makes a backup the way cron does, checks every file's checksum, restores the newest backup into a throwaway database on an internal network (row counts compared table by table with production, restored twice, migrations a no-op) and the oldest one into an empty schema followed by the platform's `migrate`, then removes the throwaway database. Nothing decrypted is written to a disk; only counts are printed.
