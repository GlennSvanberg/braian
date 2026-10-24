#!/bin/sh
set -eu

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" <<EOSQL
CREATE ROLE openpim_readonly LOGIN PASSWORD '${POSTGRES_READONLY_PASSWORD}';
GRANT pg_read_all_data TO openpim_readonly;
GRANT pg_monitor TO openpim_readonly;
EOSQL
