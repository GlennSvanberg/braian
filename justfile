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

# Bygger rules till JavaScript via WebAssembly
wasm:
    cargo build -p openpim-rules-wasm --release --target wasm32-unknown-unknown
    wasm-bindgen target/wasm32-unknown-unknown/release/openpim_rules_wasm.wasm --target nodejs --out-dir packages/rules-wasm/pkg

# Fas 1: tunga objekt valideras likadant i Rust och WebAssembly
demo-1: wasm
    cargo run -p openpim-synth --release -- demo-1 --out target/demo-1
    node packages/rules-wasm/demo-1.mjs target/demo-1

# Delade regeltestfall genom WebAssembly
test-wasm: wasm
    node --test packages/rules-wasm/test/cases.test.mjs

# Objektstorlek per nivå (lätt, medel, tung)
measure-sizes:
    cargo run -p openpim-synth --release -- sizes --count 100

# Fas 2: jämför lagringsformer. Argument skickas vidare, till exempel `--objects 300`.
spike-1 *args:
    cargo run -p openpim-bench --release -- spike-storage {{args}}

# Fas 2: ladda syntetiska objekt och visa ändringsloggen. Nivå och antal går att byta.
demo-2 tier="light" count="100000":
    cargo run -p openpim-bench --release -- demo-2 --tier {{tier}} --count {{count}}
