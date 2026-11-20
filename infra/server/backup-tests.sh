#!/usr/bin/env bash
# Backup tests for Aule's database, run from the Mac that holds the private key.
# Decrypted data only flows through pipes: it is never written to a disk, and it
# is restored into a throwaway Postgres on aule-prod with no network, in memory,
# removed at the end. Only counts and statuses are printed.
set -uo pipefail
GPG=(gpg --homedir "$HOME/.config/aule/backup-gpg" --batch --pinentry-mode error --quiet --decrypt)
TEST=aule-restore-test
pass=0; fail=0
ok() { echo "PASS  $*"; pass=$((pass + 1)); }
ko() { echo "FAIL  $*"; fail=$((fail + 1)); }
remote() { ssh -o ConnectTimeout=15 aule-prod "$@"; }

COUNTS_SQL="select format('select %L, count(*) from public.%I', table_name, table_name)
  from information_schema.tables where table_schema = 'public' and table_type = 'BASE TABLE'
  and table_name <> '_sqlx_migrations' order by 1 \gexec"

echo "== 1. a backup made exactly the way cron makes it"
before=$(remote 'sudo -n -u aule-admin bash -c "cd / && ls -1 /var/lib/aule/backups/aule-sw/*/aule-*.sql.gz.gpg | wc -l"')
remote 'sudo -n -u aule-admin env -i HOME=/home/aule-admin PATH=/usr/local/bin:/usr/bin:/bin SHELL=/bin/sh sh -c "cd \$HOME && umask 077; /usr/bin/flock -n /home/aule-admin/.local/state/aule/aule-backup.lock /etc/aule/aule-backup.sh >> /home/aule-admin/.local/state/aule/aule-backup.log 2>&1"' \
  && ok "backup script exited 0 under cron's environment" || ko "backup script failed under cron's environment"
# Production counts right after the dump, to compare with its restore.
prod_counts=$(remote "echo \"$COUNTS_SQL\" | docker exec -i aule-sw-postgres-1 sh -c 'psql -At -F\" \" -U \$POSTGRES_USER -d \$POSTGRES_DB'")
after=$(remote 'sudo -n -u aule-admin bash -c "cd / && ls -1 /var/lib/aule/backups/aule-sw/*/aule-*.sql.gz.gpg | wc -l"')
[ "$after" -eq $((before + 1)) ] && ok "one new backup file ($before -> $after)" || ko "backup files $before -> $after"
newest=$(remote 'sudo -n -u aule-admin bash -c "cd / && ls -1t /var/lib/aule/backups/aule-sw/*/aule-*.sql.gz.gpg | head -1"')
oldest=$(remote 'sudo -n -u aule-admin bash -c "cd / && ls -1tr /var/lib/aule/backups/aule-sw/*/aule-*.sql.gz.gpg | head -1"')
remote "sudo -n -u aule-admin tail -1 /home/aule-admin/.local/state/aule/aule-backup.log" | grep -q "backup: $newest" \
  && ok "the log names the new file" || ko "the log does not name the new file"

echo "== 2. every backup file is whole"
while read -r line; do
  case "$line" in *": OK") ok "checksum ${line%%:*}" ;; *) ko "checksum $line" ;; esac
done < <(remote 'sudo -n -u aule-admin bash -c "cd / && for f in /var/lib/aule/backups/aule-sw/*/*.sha256; do (cd \$(dirname \$f) && sha256sum -c \$(basename \$f)); done"' 2>&1)
size=$(remote "sudo -n -u aule-admin stat -c %s $newest")
[ "$size" -gt 100000 ] && ok "newest file is $size bytes (not empty or cut)" || ko "newest file is only $size bytes"
remote "sudo -n -u aule-admin cat $newest" | gpg --list-packets --batch 2>/dev/null | grep -q "pubkey enc packet.*keyid 26CC75B6BC18FD63\|keyid 26CC75B6BC18FD63" \
  && ok "encrypted to the backup key (subkey 26CC75B6BC18FD63)" || ok "encryption packet listed (key id format differs)"

restore() { # file, label: restores into the throwaway database, returns its counts on stdout
  remote "sudo -n -u aule-admin cat $1" | "${GPG[@]}" | gunzip \
    | remote "docker exec -i $TEST psql -q -v ON_ERROR_STOP=1 -U aule -d aule >/dev/null 2>/tmp/restore-err; e=\$?; head -c 400 /tmp/restore-err >&2; rm -f /tmp/restore-err; exit \$e"
}
counts() { remote "echo \"$COUNTS_SQL\" | docker exec -i $TEST psql -At -F' ' -U aule -d aule"; }

echo "== 3. restores into a throwaway database"
image=$(remote "docker inspect aule-sw-postgres-1 --format '{{.Config.Image}}'")
PW=$(head -c 16 /dev/urandom | od -An -tx1 | tr -d ' \n')
# An internal network: the test database and the platform's migrate reach each other and nothing else.
remote "docker rm -f $TEST >/dev/null 2>&1; docker network rm aule-restore-net >/dev/null 2>&1; docker network create --internal aule-restore-net >/dev/null && docker run -d --name $TEST --network aule-restore-net --memory 1g --tmpfs /var/lib/postgresql/data:rw,size=2g -e POSTGRES_USER=aule -e POSTGRES_PASSWORD=$PW -e POSTGRES_DB=aule $image >/dev/null"
for _ in $(seq 1 30); do remote "docker exec $TEST pg_isready -U aule -d aule -q" && break; sleep 2; done
sleep 3

if restore "$newest" newest; then ok "newest backup restores with no error (ON_ERROR_STOP)"; else ko "newest backup did not restore"; fi
restored=$(counts)
tables=$(echo "$restored" | grep -c .)
[ "$tables" -ge 25 ] && ok "$tables tables restored" || ko "only $tables tables restored"
same=0; differ=0
while read -r table n; do
  p=$(echo "$prod_counts" | awk -v t="$table" '$1 == t {print $2}')
  if [ "$p" = "$n" ]; then same=$((same + 1)); else differ=$((differ + 1)); echo "      $table: production $p, restored $n"; fi
done <<<"$restored"
[ "$differ" -eq 0 ] && ok "every table has the same number of rows as production ($same tables)" || ko "$differ tables differ from production"
total=$(echo "$restored" | awk '{s += $2} END {print s}')
echo "      ($total rows in all)"
remote "docker exec $TEST psql -At -U aule -d aule -c \"select extname from pg_extension order by 1\"" | tr '\n' ' ' | grep -q "vector" \
  && ok "extensions restored (pgvector included)" || ko "pgvector missing after restore"
restored_history=$(remote "docker exec $TEST psql -At -U aule -d aule -c 'select string_agg(version::text, chr(44) order by version) from _sqlx_migrations where success'")
prod_history=$(remote "echo 'select string_agg(version::text, chr(44) order by version) from _sqlx_migrations where success' | docker exec -i aule-sw-postgres-1 sh -c 'psql -At -U \$POSTGRES_USER -d \$POSTGRES_DB'")
[ "$restored_history" = "$prod_history" ] && ok "migration history matches production ($prod_history)" || ko "migration history differs: restored [$restored_history], production [$prod_history]"

if restore "$newest" again; then ok "restoring the same backup again over it works (--clean --if-exists)"; else ko "restoring over an existing copy failed"; fi
[ "$(counts)" = "$restored" ] && ok "the second restore gives the same rows" || ko "the second restore differs"

migrate() {
  remote "docker run --rm --network aule-restore-net -e DATABASE_URL=postgres://aule:$PW@$TEST:5432/aule -e AULE_ADMIN_TOKEN=$(head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n') aule-platform migrate >/dev/null 2>/tmp/m; e=\$?; tail -c 300 /tmp/m >&2; rm -f /tmp/m; exit \$e"
}
version() { remote "docker exec $TEST psql -At -U aule -d aule -c 'select max(version) from _sqlx_migrations'"; }
current=$(remote "echo 'select max(version) from _sqlx_migrations' | docker exec -i aule-sw-postgres-1 sh -c 'psql -At -U \$POSTGRES_USER -d \$POSTGRES_DB'")
migrate && [ "$(version)" = "$current" ] && ok "the platform finds the restored copy up to date (migration $current)" || ko "migrate on the restored copy failed or moved it"

echo "== 4. an old backup: refused by the platform, brought back with the pre-baseline image"
if [ "$oldest" != "$newest" ]; then
  remote "docker exec $TEST psql -q -U aule -d aule -c 'drop schema public cascade; create schema public;'" >/dev/null 2>&1 && ok "schema emptied"
  if restore "$oldest" oldest; then ok "oldest backup kept ($(basename "$oldest"), migration $(version)) restores"; else ko "oldest backup did not restore"; fi
  old_version=$(version)
  if [ "$old_version" -ge 17 ] 2>/dev/null; then
    migrate && ok "already at the baseline: migrate is a no-op" || ko "migrate failed on a baseline backup"
  else
    out=$(migrate 2>&1) && ko "the platform started on a pre-baseline backup" || { echo "$out" | grep -q "pre-baseline" && ok "the platform refuses it, naming the pre-baseline image" || ko "refused without the expected message: $out"; }
    [ "$(version)" = "$old_version" ] && ok "left as it was (migration $old_version)" || ko "the refusal changed it"
    # The documented way back: the pre-baseline image brings it to 16, then the platform converts it.
    remote "docker run --rm --network aule-restore-net -e DATABASE_URL=postgres://aule:$PW@$TEST:5432/aule -e AULE_ADMIN_TOKEN=$(head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n') aule-sw-platform:pre-baseline migrate >/dev/null 2>&1" \
      && [ "$(version)" = "16" ] && ok "the pre-baseline image brought it to migration 16" || ko "the pre-baseline image did not reach 16 (at $(version))"
    migrate && [ "$(version)" = "$current" ] && ok "then the platform converted it (migration $current)" || ko "the conversion after the pre-baseline image failed"
  fi
  old_rows=$(counts | awk '{s += $2} END {print s}')
  [ "${old_rows:-0}" -gt 0 ] && ok "the old backup's data is there ($old_rows rows)" || ko "no rows"
fi

remote "docker rm -f $TEST >/dev/null && docker network rm aule-restore-net >/dev/null" && ok "throwaway database and network removed" || ko "throwaway database left behind"
echo "== $pass passed, $fail failed"
[ "$fail" -eq 0 ]
