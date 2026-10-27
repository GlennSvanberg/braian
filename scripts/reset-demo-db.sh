#!/usr/bin/env bash
# Reset the demo database named by DATABASE_URL.
#
# Manual only, after an explicit decision. This script is not part of deploy,
# CI, or any systemd unit. Pass --yes. It refuses a database whose name matches
# braian-perf-*-stack. It writes a timestamped pg_dump first and stops if that
# backup fails. The password is never printed.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

confirmed=0
for arg in "$@"; do
  case "$arg" in
    --yes) confirmed=1 ;;
    *)
      echo "unknown argument: ${arg}" >&2
      echo "usage: scripts/reset-demo-db.sh --yes" >&2
      exit 2
      ;;
  esac
done

if [[ "$confirmed" -ne 1 ]]; then
  echo "refusing: pass --yes to reset the demo database" >&2
  exit 1
fi

load_env_file() {
  local file="$1" line key value
  [[ -f "$file" ]] || return 0
  while IFS= read -r line || [[ -n "$line" ]]; do
    line="${line#"${line%%[![:space:]]*}"}"
    [[ -z "$line" || "$line" == \#* ]] && continue
    [[ "$line" == export\ * ]] && line="${line#export }"
    key="${line%%=*}"
    value="${line#*=}"
    [[ "$key" =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]] || continue
    if [[ -n "${!key+x}" ]]; then
      continue
    fi
    if [[ ${#value} -ge 2 && "$value" == \"*\" && "$value" == *\" ]]; then
      value="${value:1:${#value}-2}"
    elif [[ ${#value} -ge 2 && "$value" == \'*\' && "$value" == *\' ]]; then
      value="${value:1:${#value}-2}"
    fi
    printf -v "$key" '%s' "$value"
    export "${key?}"
  done < "$file"
}

load_env_file "$root/.env"

if [[ -z "${DATABASE_URL:-}" ]]; then
  echo "DATABASE_URL is not set. Copy .env.example to .env, or export the same variable the server uses." >&2
  exit 1
fi

parse_target="$(mktemp)"
chmod 600 "$parse_target"
python3 - <<'PY' >"$parse_target"
import os
from urllib.parse import unquote, urlparse

url = urlparse(os.environ["DATABASE_URL"])
name = unquote((url.path or "/").split("?")[0].lstrip("/").split("/")[0])
host = unquote(url.hostname or "")
user = unquote(url.username or "")
password = unquote(url.password or "")
port = str(url.port or 5432)

def sh(value: str) -> str:
    return "'" + value.replace("'", "'\\''") + "'"

print(f"TARGET_DB={sh(name)}")
print(f"TARGET_HOST={sh(host)}")
print(f"PGUSER={sh(user)}")
print(f"PGPASSWORD={sh(password)}")
print(f"PGPORT={sh(port)}")
PY
# shellcheck disable=SC1090
source "$parse_target"
rm -f "$parse_target"

if [[ -z "${TARGET_DB}" || -z "${TARGET_HOST}" ]]; then
  echo "DATABASE_URL has no database name or host" >&2
  exit 1
fi

if [[ ! "$TARGET_DB" =~ ^[A-Za-z_][A-Za-z0-9_-]*$ ]]; then
  echo "refusing: database name is not a plain identifier" >&2
  exit 1
fi

if [[ "$TARGET_DB" == braian-perf-*-stack ]]; then
  echo "refusing: ${TARGET_DB} on ${TARGET_HOST} looks like a braian-perf-*-stack database" >&2
  exit 1
fi

echo "Demo database: ${TARGET_DB}"
echo "Host: ${TARGET_HOST}"
echo "Stop the API and the indexer before continuing. This script then terminates leftover sessions on ${TARGET_DB}, drops it, and creates it again."

stamp="$(date -u +%Y%m%dT%H%M%SZ)"
backup_dir="${BRAIAN_BACKUP_DIR:-$root/backups}"
mkdir -p "$backup_dir"
backup="${backup_dir}/braian-${TARGET_DB}-${stamp}.dump"

export PGHOST="$TARGET_HOST"
export PGPORT
export PGUSER
export PGPASSWORD

if ! pg_dump --format=custom --file="$backup" --host="$TARGET_HOST" --port="$PGPORT" --username="$PGUSER" --dbname="$TARGET_DB"; then
  echo "backup failed; the database was not dropped" >&2
  exit 1
fi
echo "backup: ${backup}"

# Connect to the maintenance database. dropdb refuses to drop the database it is connected to.
psql --dbname=postgres --set=ON_ERROR_STOP=1 --quiet \
  --command "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '${TARGET_DB}' AND pid <> pg_backend_pid();" \
  >/dev/null
dropdb --maintenance-db=postgres "$TARGET_DB"
createdb --maintenance-db=postgres "$TARGET_DB"

# Store::connect applies the SQL migrations. `braian seed` does that, then loads the demo catalog.
cargo run -p braian-server -- seed
cargo run -p braian-server -- reindex

echo "reset complete: ${TARGET_DB} on ${TARGET_HOST}"
