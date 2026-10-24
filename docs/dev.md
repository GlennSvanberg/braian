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
| `just demo-2` | Fas 2: ladda 100 000 lätta objekt, hämta, ändra och radera |
| `just demo-2 tier=medium count=1000` | Samma demo med annan nivå och annat antal |
| `just spike-1` | Fas 2: jämför tre lagringsformer. Körs inte i CI |
| `just psql` | psql mot utvecklingsdatabasen |
| `just rpk topic list` | rpk mot Redpanda |

## En binär, flera roller

`openpim api`, `openpim relay`, `openpim indexer` och `openpim all`.
I utveckling kör man `all`. I drift kör man varje roll som egen process och skalar dem var för sig.

`just demo-2` skriver till databasen `openpim_demo` och rör inte databasen `openpim`. Nivån är lätt som standard. Medel och tung väljs med `tier`, och antalet objekt med `count`.

`just spike-1` mäter import och ändringar för ett dokument, en del per språk, och en del per språk och attributgrupp. Standard är tung nivå, 20 000 objekt och 2 000 ändringar per ändringstyp. Argument efter kommandot skickas vidare, till exempel `just spike-1 --objects 300`. Resultatet skrivs till `target/spike-1.json`. Spiken körs inte i CI. Siffrorna är indikativa när Postgres körs i Docker.

Integrationstesterna i `crates/store` kräver `OPENPIM_TEST_DATABASE_URL`. Den ska peka på underhållsdatabasen `postgres`, till exempel `postgres://openpim:byt-mig@localhost:5433/postgres`. Varje test skapar en egen databas från en migrerad mall. Saknas variabeln avbryts testet med ett fel.

`just demo-1` behöver `wasm-bindgen` 0.2.129 på `PATH` (`cargo install wasm-bindgen-cli --version 0.2.129`).
Devcontainern installerar samma version. wasm-bindgen's Node-mål är CommonJS, så `packages/rules-wasm` har ingen `"type": "module"`; skripten är `.mjs`.

## Postgres-MCP

`.cursor/mcp.json` startar Postgres MCP Pro i läsläge mot utvecklingsdatabasen,
med rollen `openpim_readonly` (`DATABASE_URI` i `.env`). Rollen skapas när Postgres-volymen initieras första gången.
