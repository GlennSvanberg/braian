set dotenv-load := true
set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

default:
    @just --list

# Postgres, Redpanda, Redpanda Console och Typesense
up:
    docker compose up -d --wait

down:
    docker compose down

# Raderar all lokal data i Postgres, Redpanda och Typesense
nuke:
    docker compose down -v

logs *service:
    docker compose logs -f {{service}}

dev:
    cargo run -p openpim-server -- all

check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo build -p openpim-rules -p openpim-rules-wasm --target wasm32-unknown-unknown

test:
    cargo nextest run --workspace --no-tests=pass

psql:
    docker compose exec postgres psql -U {{env("POSTGRES_USER")}} -d {{env("POSTGRES_DB")}}

rpk *args:
    docker compose exec redpanda rpk {{args}}
