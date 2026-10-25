# Utvecklingsmiljö

## Kom igång

1. Kopiera `.env.example` till `.env` och byt alla värden. `.env` är gitignorerad.
2. Öppna repot i devcontainern (Cursor/VS Code: "Reopen in Container").
   Postgres, Redpanda, Redpanda Console och Typesense startar automatiskt.
3. `pnpm install` en gång, sedan `just dev`. `just` listar alla kommandon.

På Windows går bind mounts från `C:\` långsamt. Klona hellre repot i WSL-filsystemet,
eller använd "Clone Repository in Container Volume". `target/` ligger alltid i en egen volym.

`just` installeras med `winget install --id Casey.Just` eller `scoop install just`.
Saknas `just` kör `.\scripts\up.ps1` och `.\scripts\dev.ps1` samma sak.
`rdkafka` byggs med CMake (`cmake-build`). Visual Studio Build Tools, workload
"Desktop development with C++", ger CMake och kompilatorn. `winget install --id Kitware.CMake`
räcker inte ensamt.

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
| `just dev` | Migrationer, demodata, API, relay, indexer och webbgränssnittet |
| `just check` | Format, clippy, och att `rules` kompilerar till WebAssembly |
| `just test` | Alla tester |
| `just demo-1` | Fas 1: generera tunga objekt och validera dem i Rust och WebAssembly |
| `just test-wasm` | Kör `testdata/rules/` i Node genom WebAssembly-bygget |
| `just measure-sizes` | Mät objektstorlek per nivå |
| `just demo-2` | Fas 2: ladda 100 000 lätta objekt, hämta, ändra och radera |
| `just demo-2 tier=medium count=1000` | Samma demo med annan nivå och annat antal |
| `just spike-1` | Fas 2: jämför tre lagringsformer. Körs inte i CI |
| `just demo-3` | Fas 3: starta API:t och kör Hurl-filerna i `tests/hurl/phase-3` |
| `just openapi` | Skriv om `docs/api/openapi.json` från koden |
| `just psql` | psql mot utvecklingsdatabasen |
| `just rpk topic list` | rpk mot Redpanda |

## En binär, flera roller

`braian api`, `braian relay`, `braian indexer` och `braian all`.
I utveckling kör man `all`. I drift kör man varje roll som egen process och skalar dem var för sig.

`just demo-2` skriver till databasen `braian_demo` och rör inte databasen `braian`. Nivån är lätt som standard. Medel och tung väljs med `tier`, och antalet objekt med `count`.

`just demo-3` skapar databasen `braian_demo3`, startar API:t på en ledig port och kör `hurl --test` mot `tests/hurl/phase-3`. Hurl 8.0.1 ska finnas på `PATH`. På Windows: `winget install Orange-OpenSource.Hurl`.

`braian api` läser `DATABASE_URL`, `BRAIAN_DEV_TOKENS` och `BRAIAN_API_ADDR` (standard `127.0.0.1:8090`, eftersom Redpanda Console tar 8080). `braian openapi` skriver OpenAPI-dokumentet. `braian all` kör migrationer, demodata, relay, indexer, live-ström, usage-konsumenten och API när `KAFKA_BROKERS` är satt. `braian telemetry` är samma konsument ensam. Den läser bara `braian.telemetry.ui`, aldrig ändringsämnena. `POST /v1/telemetry` köar och släpper vid full kö. Klassaren är av tills `BRAIAN_TELEMETRY_CLASSIFIER=1` och `OPENAI_API_KEY` båda är satta. Nyckeln läses från miljön och ska inte in i git. `just dev` startar dessutom webbgränssnittet på http://127.0.0.1:3000. Demokontot kommer från `BRAIAN_DEMO_EMAIL` och `BRAIAN_DEMO_PASSWORD` i `.env`.

`just spike-1` mäter import och ändringar för ett dokument, en del per språk, och en del per språk och attributgrupp. Standard är tung nivå, 20 000 objekt och 2 000 ändringar per ändringstyp. Argument efter kommandot skickas vidare, till exempel `just spike-1 --objects 300`. Resultatet skrivs till `target/spike-1.json`. Spiken körs inte i CI. Siffrorna är indikativa när Postgres körs i Docker.

Integrationstesterna i `crates/store` kräver `BRAIAN_TEST_DATABASE_URL`. Den ska peka på underhållsdatabasen `postgres`, till exempel `postgres://braian:byt-mig@localhost:5433/postgres`. Varje test skapar en egen databas från en migrerad mall. Saknas variabeln avbryts testet med ett fel.

`just demo-1` behöver `wasm-bindgen` 0.2.129 på `PATH` (`cargo install wasm-bindgen-cli --version 0.2.129`).
Devcontainern installerar samma version. wasm-bindgen's Node-mål är CommonJS, så `packages/rules-wasm` har ingen `"type": "module"`; skripten är `.mjs`.

## Replikeringsslot och delad Postgres

`max_slot_wal_keep_size` gäller hela Postgres-instansen, inte en databas. Slotten
bekräftar hur långt relayn har läst. WAL som ligger framför den punkten får inte
återvinnas. När taket nås invaliderar Postgres slotten (`wal_status = lost`,
`invalidation_reason = wal_removed`). Samma checkpoint kan ta med sig andra
logiska slottar på instansen, även för en databas som är inaktiv, eftersom de
delar `pg_wal`.

Relayn läser `pg_replication_slots` varje sekund. `GET /metrics` på
`BRAIAN_RELAY_ADDR` (standard `127.0.0.1:8091`) exponerar:

| Mätare | Betydelse |
| --- | --- |
| `braian_relay_lag_bytes` | `pg_current_wal_lsn()` minus `confirmed_flush_lsn` |
| `braian_relay_lag_exceeded` | 1 när laggen är över `BRAIAN_RELAY_LAG_LIMIT_BYTES` (standard 64 MiB) |
| `braian_relay_slot_wal_status` | 0 saknas, 1 `reserved`, 2 `extended`, 3 `unreserved`, 4 `lost` |
| `braian_relay_slot_invalidated` | 1 när slotten är `lost` eller har en `invalidation_reason` |
| `braian_relay_slot_recoveries_total` | Antal gånger relayn har släppt en ogiltig slot och bett om omindexering |

`GET /healthz` på samma port svarar 204 när slotten finns, inte är `lost` eller
`unreserved`, och laggen är under gränsen. Annars 503.

När slotten är ogiltig loggar relayn det, gör `pg_drop_replication_slot`, sätter
`indexer_progress.reindex_requested` och skapar en ny slot. Indexern bygger då
ett nytt Typesense-alias från Postgres. Hålet i WAL går inte att spela upp;
Postgres är källan. Relayn rör bara sin egen slot.

### Delad instans

Flera Braian, eller Braian bredvid något annat, på samma Postgres:

- Sätt `max_slot_wal_keep_size` så att den långsammaste slotten ryms, eller `-1`
  om disken övervakas och en full disk är ett känt fel. 10 GB räcker inte för en
  stor massinläsning om relayn eller indexern ligger efter: skrivningen på en
  databas kan då slå ut en idle slot på en annan.
- En slot per databas (`BRAIAN_RELAY_SLOT`). Olika installationer ska inte dela namn.
- Larma på `braian_relay_lag_bytes`, `braian_relay_slot_invalidated` och 503 från
  relayns `/healthz`. API:ts `/healthz` säger bara att processen lever.

### Egen instans

En Postgres som bara kör en Braian kan behålla taket från `compose.yaml`
(`max_slot_wal_keep_size=10GB`, `max_wal_size=8GB`, `wal_compression=lz4`).
Relayn återskapar sin slot och ber om omindexering om taket ändå nås. Det tar
inte med sig någon annan installation.

## Interaktiv trafik och massinläsning

API:t har två Postgres-pooler. Interaktiva läsningar och skrivningar tar
`BRAIAN_POOL_INTERACTIVE` (standard 24). Massinläsning, bulk-jobb och indexern
tar `BRAIAN_POOL_BULK` (standard 8), så att en COPY inte kan ta alla anslutningar.
Åtta räcker till benchens standard på åtta loaders. Sätt `12` på en värd där
importen ska använda mer av maskinen; det var poolstorleken som återställde
bulk-takten när fyra anslutningar höll igen den.

Varje pågående interaktiv läsning eller skrivning i API-processen räknas.
Hälsokontroll, OpenAPI, SSE-strömmen och själva `POST /v1/bulk` räknas inte:
en öppen flik skulle annars pausa importen hela tiden. Innan nästa bulk-slice
hämtar en anslutning tittar jobbet på räkningen. Är den noll väntar jobbet
`BRAIAN_BULK_YIELD_MS` (standard 1, `0` yieldar bara). Är den över noll växer
väntan med `BRAIAN_BULK_YIELD_STEP_MS` per pågående anrop (standard 4) och
stannar vid `BRAIAN_BULK_YIELD_MAX_MS` (standard 25). Slice:n blir också
mindre: konfigurerad storlek delat med `1 + pågående anrop`, minst en rad och
högst 32. Taket gör att en lång klickström saktar bulken men inte parkerar den.

Bulk-slice:ar sätter `synchronous_commit = off` inuti transaktionen. Slice:n
är fortfarande atomär. Jobbets nästa framstegsskrivning är en vanlig commit
och spolar då även den föregående slice:ns WAL. En krasch kan tappa den sista
slice som inte hunnit bli durable, inte halva slice:n.

`BRAIAN_BULK_JOBS` (standard 1) är hur många bulk-jobb som kör, och
`BRAIAN_BULK_QUEUE` (standard 4) hur många som får vänta. Fler får 429.
`BRAIAN_BULK_CHUNK` (standard 25) är raderna per transaktion i `POST /v1/bulk`
när ingen interaktiv trafik pågår. `braian-bench perf-load` använder samma
standardvärden som koden, inte miljövariablerna, så att en export i skalet
inte ändrar en mätning av misstag.

`POST /v1/objects:batchGet` läser upp till 200 id. Utan `values`, `fields`
eller `parts` kommer bara huvudet och grupperna. `fields` är attributnycklar;
`languages` begränsar språkberoende fält till de delarna.

## Typesense och indexern på en 8-kärnig värd

`deploy/compose.perf.yaml` är ett overlay för en värd med 8 vCPU, samma klass
som perf-körningarna. Det ger Typesense två egna kärnor och lastgeneratorn en.
Dev-compose (`just up`) använder det inte.

```
docker compose -f compose.yaml -f deploy/compose.perf.yaml up -d postgres redpanda typesense
```

| Kärnor | Process |
| --- | --- |
| 0–2 | Postgres |
| 3 | `taskset -c 3` på `braian api`, `braian relay` och `braian indexer` |
| 4 | Redpanda (`--smp=1` i dev-compose) |
| 5 | lastgeneratorn (`perf`-profilen, eller `taskset -c 5`) |
| 6–7 | Typesense, `--thread-pool-size=2` |

Indexern har två spår. Det interaktiva spåret använder
`BRAIAN_INDEXER_BATCH` (standard 200), `BRAIAN_INDEXER_IMPORT_BATCH`
(standard 100) och `BRAIAN_INDEXER_IMPORT_CONCURRENCY` (standard 4).
Bulk-spåret väntar tills det interaktiva topicet är tömt innan det importerar,
upp till `BRAIAN_INDEXER_BULK_YIELD_MAX_MS` (standard 100). Därefter importerar
det ändå, så att en jämn ström av ändringar inte svälter massinläsningen. Bulk läser
`BRAIAN_INDEXER_BULK_BATCH` händelser (standard 2 000) och importerar dem som
`BRAIAN_INDEXER_BULK_IMPORT_BATCH` dokument per anrop (standard 500) med
`BRAIAN_INDEXER_BULK_IMPORT_CONCURRENCY` anrop igång (standard 4). Det är fyra
anrop på 500 dokument, i stället för små anrop på 100, och är det som lyfter
taket mot Typesense. På layouten ovan, sätt bulk-samtidigheten till 2 så den
följer Typesense-trådarna. Omindexering använder bulk-storlekarna.

På en maskin med färre kärnor, ändra `cpuset` i overlayet innan det startas.
En kärna som inte finns gör att containern inte startar.

## Postgres-MCP

`.cursor/mcp.json` startar Postgres MCP Pro i läsläge mot utvecklingsdatabasen,
med rollen `braian_readonly` (`DATABASE_URI` i `.env`). Rollen skapas när Postgres-volymen initieras första gången.
