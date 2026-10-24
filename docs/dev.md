# Utvecklingsmiljö

## Kom igång

1. Kopiera `.env.example` till `.env` och byt alla värden. `.env` är gitignorerad.
2. Öppna repot i devcontainern (Cursor/VS Code: "Reopen in Container").
   Postgres, Redpanda, Redpanda Console och Typesense startar automatiskt.
3. `just` listar alla kommandon.

På Windows går bind mounts från `C:\` långsamt. Klona hellre repot i WSL-filsystemet,
eller använd "Clone Repository in Container Volume". `target/` ligger alltid i en egen volym.

## Tjänster

| Tjänst | Från värddatorn | Från devcontainern |
| --- | --- | --- |
| Postgres | `localhost:5433` (`POSTGRES_HOST_PORT`) | `postgres:5432` |
| Redpanda (Kafka) | `localhost:19092` | `redpanda:9092` |
| Redpanda schema registry | `localhost:18081` | `redpanda:8081` |
| Redpanda Console | http://localhost:8080 | – |
| Typesense | `localhost:8108` | `typesense:8108` |

## Kommandon

| Kommando | Vad |
| --- | --- |
| `just up` / `just down` | Starta och stoppa infrastrukturen |
| `just nuke` | Radera all lokal data |
| `just dev` | Kör `openpim all` (alla roller i en process) |
| `just check` | Format, clippy, och att `rules` kompilerar till WebAssembly |
| `just test` | Alla tester |
| `just demo-1` | Fas 1: generera tunga objekt och validera dem i Rust och WebAssembly |
| `just test-wasm` | Kör `testdata/rules/` i Node genom WebAssembly-bygget |
| `just measure-sizes` | Mät objektstorlek per nivå |
| `just psql` | psql mot utvecklingsdatabasen |
| `just rpk topic list` | rpk mot Redpanda |

## En binär, flera roller

`openpim api`, `openpim relay`, `openpim indexer` och `openpim all`.
I utveckling kör man `all`. I drift kör man varje roll som egen process och skalar dem var för sig.

`just demo-1` behöver `wasm-bindgen` 0.2.129 på `PATH` (`cargo install wasm-bindgen-cli --version 0.2.129`).
Devcontainern installerar samma version. wasm-bindgen's Node-mål är CommonJS, så `packages/rules-wasm` har ingen `"type": "module"`; skripten är `.mjs`.

## Postgres-MCP

`.cursor/mcp.json` startar Postgres MCP Pro i läsläge mot utvecklingsdatabasen,
med rollen `openpim_readonly` (`DATABASE_URI` i `.env`). Rollen skapas när Postgres-volymen initieras första gången.
