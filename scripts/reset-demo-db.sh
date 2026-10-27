#!/usr/bin/env bash
# Reset one local demo database named by DATABASE_URL.
#
# Manual only, after an explicit decision. This script is not part of deploy,
# CI, or any systemd unit. Pass --yes.
#
# It refuses anything except the allowlisted database names, refuses
# braian-perf-*-stack, and refuses a host that is not loopback, the selected
# container, or listed in BRAIAN_RESET_ALLOWED_HOSTS. It takes a timestamped
# pg_dump first and stops if that backup fails. The password is never printed.
#
# Postgres on the demo host is the container braian-postgres-1 (postgres:18).
# The host has no client tools. pg_dump, the session check, dropdb and createdb
# then run with docker exec, and the dump is streamed to a file on the host.
# Without a container, local clients must be major version 18.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

usage() {
  cat <<'EOF'
usage: scripts/reset-demo-db.sh --yes [--container NAME]

Resets the database in DATABASE_URL. Manual only. Stop braian, braian-web and
braian-autodeploy.timer first. Other sessions on the database abort the run.

  --yes                 required
  --container NAME      docker container for pg_dump, psql, dropdb and createdb

Environment:
  DATABASE_URL                  target (or the same key in .env)
  BRAIAN_PG_CONTAINER           container name (default braian-postgres-1 when it is running)
  BRAIAN_RESET_ALLOWED_DBS      comma-separated names (default braian, braian_dev, braian_test)
  BRAIAN_RESET_ALLOWED_HOSTS    extra hosts, or host:port, comma-separated
  BRAIAN_BIN                    seed binary (default target/release/braian, else cargo run)
  BRAIAN_BACKUP_DIR             dump directory (default ./backups, mode 0700)
EOF
}

confirmed=0
container_from_flag=0
container_flag=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --yes) confirmed=1 ;;
    --container)
      shift
      if [[ $# -eq 0 || -z "${1:-}" ]]; then
        echo "missing container name after --container" >&2
        usage >&2
        exit 2
      fi
      container_flag="$1"
      container_from_flag=1
      ;;
    --container=*)
      container_flag="${1#--container=}"
      if [[ -z "$container_flag" ]]; then
        echo "missing container name after --container=" >&2
        usage >&2
        exit 2
      fi
      container_from_flag=1
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: ${1}" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
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

if [[ -z "${PGUSER}" ]]; then
  echo "DATABASE_URL has no user" >&2
  exit 1
fi

if [[ ! "$PGPORT" =~ ^[0-9]+$ ]] || (( PGPORT < 1 || PGPORT > 65535 )); then
  echo "refusing: port ${PGPORT} is not a TCP port" >&2
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

allowed_dbs=()
if [[ -n "${BRAIAN_RESET_ALLOWED_DBS:-}" ]]; then
  IFS=', ' read -r -a allowed_dbs <<< "$BRAIAN_RESET_ALLOWED_DBS"
else
  allowed_dbs=(braian braian_dev braian_test)
fi
db_allowed=0
for name in "${allowed_dbs[@]}"; do
  [[ -z "$name" ]] && continue
  if [[ "$name" == "$TARGET_DB" ]]; then
    db_allowed=1
    break
  fi
done
if [[ "$db_allowed" -ne 1 ]]; then
  echo "refusing: database ${TARGET_DB} is not allowlisted. Allowed: ${allowed_dbs[*]:-none} (BRAIAN_RESET_ALLOWED_DBS replaces the default braian, braian_dev, braian_test)." >&2
  exit 1
fi

# 0 running, 1 not running, 2 docker could not answer.
container_running() {
  local name="$1" state err
  err="$(mktemp)"
  chmod 600 "$err"
  state="$(docker inspect -f '{{.State.Running}}' "$name" 2>"$err" || true)"
  if [[ "$state" == "true" ]]; then
    rm -f "$err"
    CONTAINER_INSPECT_ERROR=""
    return 0
  fi
  if [[ -s "$err" ]]; then
    CONTAINER_INSPECT_ERROR="$(tr '\n' ' ' <"$err")"
    rm -f "$err"
    return 2
  fi
  rm -f "$err"
  CONTAINER_INSPECT_ERROR=""
  return 1
}

PG_CONTAINER=""
if [[ "$container_from_flag" -eq 1 ]]; then
  PG_CONTAINER="$container_flag"
elif [[ -n "${BRAIAN_PG_CONTAINER:-}" ]]; then
  PG_CONTAINER="$BRAIAN_PG_CONTAINER"
elif command -v docker >/dev/null 2>&1 && container_running braian-postgres-1; then
  PG_CONTAINER="braian-postgres-1"
fi

is_loopback() {
  case "$1" in
    localhost|127.0.0.1|::1) return 0 ;;
    *) return 1 ;;
  esac
}

host_allowed() {
  local host="$1" port="$2" entry bare h p
  if is_loopback "$host"; then
    return 0
  fi
  if [[ -n "$PG_CONTAINER" && "$host" == "$PG_CONTAINER" ]]; then
    return 0
  fi
  local -a entries=()
  IFS=', ' read -r -a entries <<< "${BRAIAN_RESET_ALLOWED_HOSTS:-}"
  for entry in "${entries[@]}"; do
    [[ -z "$entry" ]] && continue
    if [[ "$entry" == \[*\]:* ]]; then
      p="${entry##*:}"
      h="${entry%:*}"
      h="${h#[}"
      h="${h%]}"
      if [[ "$host" == "$h" && "$port" == "$p" ]]; then
        return 0
      fi
    elif [[ "$entry" == *:* && "$entry" != *:*:* ]]; then
      h="${entry%%:*}"
      p="${entry#*:}"
      if [[ "$host" == "$h" && "$port" == "$p" ]]; then
        return 0
      fi
    else
      bare="${entry#[}"
      bare="${bare%]}"
      if [[ "$host" == "$entry" || "$host" == "$bare" ]]; then
        return 0
      fi
    fi
  done
  return 1
}

if ! host_allowed "$TARGET_HOST" "$PGPORT"; then
  echo "refusing: ${TARGET_HOST}:${PGPORT} is not an allowed connection. Allowed hosts are localhost, 127.0.0.1, ::1, the selected container, and BRAIAN_RESET_ALLOWED_HOSTS." >&2
  exit 1
fi

require_units_stopped() {
  if ! command -v systemctl >/dev/null 2>&1; then
    return 0
  fi
  local unit output
  local -a active=()
  for unit in braian braian-web braian-autodeploy.timer; do
    output="$(systemctl is-active "$unit" 2>&1 || true)"
    case "$output" in
      active|activating|reloading)
        active+=("$unit")
        ;;
      inactive|failed|deactivating|unknown|dead)
        ;;
      *"not been booted with systemd"*|*"Failed to connect to bus"*)
        # systemctl is installed, but this host has no systemd to query.
        return 0
        ;;
      *)
        echo "refusing: systemctl could not report whether ${unit} is active (${output:-no output}). Stop braian, braian-web and braian-autodeploy.timer before resetting." >&2
        exit 1
        ;;
    esac
  done
  if [[ ${#active[@]} -gt 0 ]]; then
    echo "refusing: stop these units before backup and drop. braian uses Restart=always, so a reset while it is active would reopen the database: ${active[*]}" >&2
    echo "stop them with: sudo systemctl stop braian braian-web braian-autodeploy.timer" >&2
    exit 1
  fi
}

postgres_major() {
  local line="$1" major=""
  if [[ "$line" =~ \(PostgreSQL\)[[:space:]]+([0-9]+) ]]; then
    major="${BASH_REMATCH[1]}"
  fi
  printf '%s' "$major"
}

require_major_18() {
  local label="$1" line="$2" major
  major="$(postgres_major "$line")"
  if [[ -z "$major" ]]; then
    echo "refusing: could not read a PostgreSQL major version from ${label} (${line:-no output}). Install client major version 18, or use a postgres:18 container via --container or BRAIAN_PG_CONTAINER." >&2
    exit 1
  fi
  if [[ "$major" != "18" ]]; then
    echo "refusing: ${label} is PostgreSQL ${major}. This script needs client major version 18 so the dump matches the server. Install the 18 client, or use a postgres:18 container via --container or BRAIAN_PG_CONTAINER." >&2
    exit 1
  fi
}

run_pg() {
  if [[ -n "$PG_CONTAINER" ]]; then
    docker exec -e PGPASSWORD -e PGUSER "$PG_CONTAINER" "$@"
  else
    "$@"
  fi
}

if [[ -n "$PG_CONTAINER" ]]; then
  if ! command -v docker >/dev/null 2>&1; then
    echo "refusing: container ${PG_CONTAINER} was requested but docker is not on PATH" >&2
    exit 1
  fi
  inspect_status=0
  container_running "$PG_CONTAINER" || inspect_status=$?
  if [[ "$inspect_status" -eq 2 ]]; then
    echo "refusing: could not inspect container ${PG_CONTAINER}: ${CONTAINER_INSPECT_ERROR}" >&2
    exit 1
  fi
  if [[ "$inspect_status" -ne 0 ]]; then
    echo "refusing: container ${PG_CONTAINER} is not running. Start it, or unset BRAIAN_PG_CONTAINER and install PostgreSQL client major version 18." >&2
    exit 1
  fi
fi

require_units_stopped

conn_args=()
if [[ -n "$PG_CONTAINER" ]]; then
  # Inside the container Postgres listens on 5432. The host port in DATABASE_URL
  # is the published port and is the wrong target for docker exec.
  conn_args=(--host=127.0.0.1 --port=5432 --username="$PGUSER" --no-password)
  require_major_18 "pg_dump in ${PG_CONTAINER}" "$(run_pg pg_dump --version 2>/dev/null || true)"
else
  local_missing=()
  for bin in pg_dump psql dropdb createdb; do
    if ! command -v "$bin" >/dev/null 2>&1; then
      local_missing+=("$bin")
    fi
  done
  if [[ ${#local_missing[@]} -gt 0 ]]; then
    echo "refusing: missing local client tools: ${local_missing[*]}. Install PostgreSQL client major version 18, or run via docker exec (--container or BRAIAN_PG_CONTAINER; default braian-postgres-1 when that container is running)." >&2
    exit 1
  fi
  for bin in pg_dump psql dropdb createdb; do
    require_major_18 "$bin" "$("$bin" --version 2>/dev/null || true)"
  done
  conn_args=(--host="$TARGET_HOST" --port="$PGPORT" --username="$PGUSER" --no-password)
fi

export PGUSER
export PGPASSWORD

# Read-only. Own backend is connected to `postgres`, not the target database.
session_report() {
  local sql
  sql="SELECT format('%s pid=%s user=%s app=%s state=%s', backend_type, pid, usename, COALESCE(application_name, ''), COALESCE(state, '')) FROM pg_stat_activity WHERE datname = '${TARGET_DB}' AND pid <> pg_backend_pid();"
  run_pg psql "${conn_args[@]}" --dbname=postgres --set=ON_ERROR_STOP=1 --no-align --tuples-only --quiet --command "$sql"
}

refuse_other_sessions() {
  local report
  if ! report="$(session_report)"; then
    echo "could not read pg_stat_activity for ${TARGET_DB}; the database was not dropped" >&2
    exit 1
  fi
  if [[ -n "${report//[[:space:]]/}" ]]; then
    echo "refusing: other sessions are connected to ${TARGET_DB}. Stop them and run again. This script does not terminate sessions." >&2
    printf '%s\n' "$report" >&2
    exit 1
  fi
}

refuse_other_sessions

if [[ -n "$PG_CONTAINER" ]]; then
  echo "Demo database: ${TARGET_DB}"
  echo "Host: ${TARGET_HOST}:${PGPORT}"
  echo "Container: ${PG_CONTAINER} (clients run inside the container and connect to 127.0.0.1:5432)"
else
  echo "Demo database: ${TARGET_DB}"
  echo "Host: ${TARGET_HOST}:${PGPORT}"
  echo "Container: none (local PostgreSQL 18 clients)"
fi

# Owner-only backup. umask covers the directory and the file created below.
umask 077
backup_dir="${BRAIAN_BACKUP_DIR:-$root/backups}"
if [[ -z "$backup_dir" || "$backup_dir" == "/" ]]; then
  echo "refusing: backup directory is unsafe" >&2
  exit 1
fi
mkdir -p "$backup_dir"
chmod 700 "$backup_dir"
stamp="$(date -u +%Y%m%dT%H%M%SZ)"
backup="${backup_dir}/braian-${TARGET_DB}-${stamp}.dump"

backup_ok=0
if [[ -n "$PG_CONTAINER" ]]; then
  if run_pg pg_dump --format=custom "${conn_args[@]}" --dbname="$TARGET_DB" >"$backup"; then
    backup_ok=1
  fi
else
  if run_pg pg_dump --format=custom "${conn_args[@]}" --dbname="$TARGET_DB" --file="$backup"; then
    backup_ok=1
  fi
fi
if [[ "$backup_ok" -ne 1 || ! -s "$backup" ]]; then
  rm -f "$backup"
  echo "backup failed; the database was not dropped" >&2
  exit 1
fi
chmod 600 "$backup"
echo "backup: ${backup}"

# A session may have connected while the dump ran. Check again before drop.
refuse_other_sessions

if ! run_pg dropdb "${conn_args[@]}" --maintenance-db=postgres "$TARGET_DB"; then
  echo "dropdb failed; ${TARGET_DB} was not recreated. Backup: ${backup}" >&2
  exit 1
fi
if ! run_pg createdb "${conn_args[@]}" --maintenance-db=postgres "$TARGET_DB"; then
  echo "createdb failed after drop. Backup: ${backup}" >&2
  exit 1
fi

# `braian seed` connects, applies migrations, loads the catalog, and reindexes
# when Typesense is configured. A separate reindex command would repeat that.
seed_failed() {
  echo "seed failed. The database was recreated and has no catalog. Backup: ${backup}" >&2
  exit 1
}
if [[ -n "${BRAIAN_BIN:-}" ]]; then
  seed_bin="$BRAIAN_BIN"
elif [[ -x "$root/target/release/braian" ]]; then
  seed_bin="$root/target/release/braian"
else
  seed_bin=""
fi
if [[ -n "$seed_bin" ]]; then
  if [[ ! -x "$seed_bin" ]]; then
    echo "seed binary is not executable: ${seed_bin}. The database was recreated and not seeded. Backup: ${backup}" >&2
    exit 1
  fi
  "$seed_bin" seed || seed_failed
else
  echo "target/release/braian is missing; falling back to cargo run -p braian-server -- seed"
  cargo run -p braian-server -- seed || seed_failed
fi

echo "reset complete: ${TARGET_DB} on ${TARGET_HOST}:${PGPORT}"
