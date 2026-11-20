#!/usr/bin/env bash
# Backup of Aule's database, every six hours, (families, children, their learning
# history and memories). The dump is encrypted to a public key before it
# touches the disk; the private key is not on this server, so a copy of these
# files is useless to whoever holds it without that key.
#
#   /etc/aule/aule-backup.sh
#
# Files land in /var/lib/aule/backups/aule-sw/YYYY-MM-DD/ and are kept for
# BACKUP_KEEP_DAYS (14). The home server pulls them from there every night.
# Tested by infra/server/backup-tests.sh (from the Mac that holds the key).
# To restore: gpg --decrypt FILE | gunzip | psql (see /etc/aule/RESTORE.md).
set -euo pipefail
umask 077
KEEP_DAYS="${BACKUP_KEEP_DAYS:-14}"
ROOT=/var/lib/aule/backups/aule-sw
KEY=/etc/aule/aule-backup-public-key.asc
DAY=$(date -u +%F)
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
mkdir -p "$ROOT/$DAY"
OUT="$ROOT/$DAY/aule-$STAMP.sql.gz.gpg"
PART="$OUT.part"
trap 'rm -f "$PART"' EXIT

# --clean --if-exists: the dump restores over a database that already has the
# schema. No owners or privileges: the user may differ where it is restored.
# All of it at the lowest priority, the dump inside the container too, so a
# backup never takes CPU or disk from a session in progress.
/etc/aule/aule-compose.sh exec -T postgres \
  nice -n 19 pg_dump -U aule --clean --if-exists --no-owner --no-privileges aule </dev/null \
  | nice -n 19 ionice -c 3 gzip -9 \
  | nice -n 19 ionice -c 3 gpg --batch --yes --no-tty --trust-model always --recipient-file "$KEY" --encrypt --output "$PART"

# An empty or cut-off dump is not a backup.
if [ "$(stat -c %s "$PART")" -lt 4096 ]; then
  echo "$(date -u +%FT%TZ) dump suspiciously small: $OUT" >&2
  exit 1
fi
mv "$PART" "$OUT"
(cd "$ROOT/$DAY" && sha256sum "$(basename "$OUT")" > "$(basename "$OUT").sha256")
echo "$(date -u +%FT%TZ) backup: $OUT ($(du -h "$OUT" | cut -f1))"

find "$ROOT" -mindepth 1 -maxdepth 1 -type d -mtime "+$KEEP_DAYS" -exec rm -rf {} + 2>/dev/null || true
