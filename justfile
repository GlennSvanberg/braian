set dotenv-load := true
set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

default:
    @just --list

# Postgres och Typesense
up:
    docker compose up -d --wait

down:
    docker compose down

# Raderar all lokal data i Postgres och Typesense
nuke:
    docker compose down -v

logs *service:
    docker compose logs -f {{service}}

# Migrationer, demodata, API, indexer och webbgränssnittet på http://127.0.0.1:3000
dev:
    node scripts/dev.mjs

check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo build -p braian-rules -p braian-rules-wasm --target wasm32-unknown-unknown

test:
    cargo nextest run --workspace --no-tests=pass

psql:
    docker compose exec postgres psql -U {{env("POSTGRES_USER")}} -d {{env("POSTGRES_DB")}}

# Bygger rules till JavaScript via WebAssembly
wasm:
    cargo build -p braian-rules-wasm --release --target wasm32-unknown-unknown
    wasm-bindgen target/wasm32-unknown-unknown/release/braian_rules_wasm.wasm --target nodejs --out-dir packages/rules-wasm/pkg

# Fas 1: tunga objekt valideras likadant i Rust och WebAssembly
demo-1: wasm
    cargo run -p braian-synth --release -- demo-1 --out target/demo-1
    node packages/rules-wasm/demo-1.mjs target/demo-1

# Delade regeltestfall genom WebAssembly
test-wasm: wasm
    node --test packages/rules-wasm/test/cases.test.mjs

# Objektstorlek per nivå (lätt, medel, tung)
measure-sizes:
    cargo run -p braian-synth --release -- sizes --count 100

# Fas 2: jämför lagringsformer. Argument skickas vidare, till exempel `--objects 300`.
spike-1 *args:
    cargo run -p braian-bench --release -- spike-storage {{args}}

# Fas 2: ladda syntetiska objekt och visa ändringsloggen. Nivå och antal går att byta.
demo-2 tier="light" count="100000":
    cargo run -p braian-bench --release -- demo-2 --tier {{tier}} --count {{count}}

# Fas 3: Hurl mot API:t i en ny databas, braian_demo3.
demo-3:
    cargo run -p braian-bench --release -- demo-3

# Skriv docs/api/openapi.json från koden.
openapi:
    cargo run -p braian-server -- openapi --out docs/api/openapi.json

# Fas 3: HTTP-latens mot ett direkt store-anrop, tung nivå.
api-latency:
    cargo run -p braian-bench --release -- api-latency

# Fas 4: en krasch spelar upp ocommittat indexarbete, och två arbetare tar inte samma seq.
demo-4:
    cargo test -p braian-store --test change_log -- --test-threads=1 --nocapture

# Fas 5: dubbletter och en sen händelse ger samma index, och reindex byter alias.
demo-5:
    cargo test -p braian-indexer --test converge -- --test-threads=1 --nocapture

# Statisk sida för braian.io. Resultatet ligger i apps/landing/dist.
landing:
    pnpm --dir apps/landing build
