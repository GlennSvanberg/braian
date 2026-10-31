# Utvecklingsmiljö

## Kom igång

1. Kopiera `.env.example` till `.env` och byt alla värden. `.env` är gitignorerad.
2. Öppna repot i devcontainern (Cursor/VS Code: "Reopen in Container").
   Postgres och Typesense startar automatiskt.
3. `pnpm install` en gång, sedan `just dev`. `just` listar alla kommandon.

På Windows går bind mounts från `C:\` långsamt. Klona hellre repot i WSL-filsystemet,
eller använd "Clone Repository in Container Volume". `target/` ligger alltid i en egen volym.

`just` installeras med `winget install --id Casey.Just` eller `scoop install just`.
Saknas `just` kör `.\scripts\up.ps1` och `.\scripts\dev.ps1` samma sak.

## Tjänster

| Tjänst | Från värddatorn | Från devcontainern |
| --- | --- | --- |
| Postgres | `localhost:5433` (`POSTGRES_HOST_PORT`) | `postgres:5432` |
| Typesense | `localhost:8108` | `typesense:8108` |

## Kommandon

| Kommando | Vad |
| --- | --- |
| `just up` / `just down` | Starta och stoppa infrastrukturen |
| `just nuke` | Radera all lokal data |
| `just dev` | Migrationer, demodata, API, indexer och webbgränssnittet |
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

## En binär, flera roller

`braian api`, `braian indexer` och `braian all`.
I utveckling kör man `all`. I drift kör man varje roll som egen process och skalar dem var för sig.

`just demo-2` skriver till databasen `braian_demo` och rör inte databasen `braian`. Nivån är lätt som standard. Medel och tung väljs med `tier`, och antalet objekt med `count`.

`just demo-3` skapar databasen `braian_demo3`, startar API:t på en ledig port och kör `hurl --test` mot `tests/hurl/phase-3`. Hurl 8.0.1 ska finnas på `PATH`. På Windows: `winget install Orange-OpenSource.Hurl`.

`scripts/reset-demo-db.sh` återställer demodatabasen i `DATABASE_URL`. Det körs bara manuellt efter ett beslut. Det är inte inkopplat i deploy, CI eller någon systemd-unit. Kräv `--yes`. Stoppa `braian`, `braian-web` och `braian-autodeploy.timer` först (`braian` har `Restart=always`). Där `systemctl` finns avbryter skriptet om någon av de enheterna är aktiv. Det avbryter också om `pg_stat_activity` visar en annan session mot databasen, och avslutar inte sessioner själv.

Tillåtna namn är `braian`, `braian_dev` och `braian_test`. `BRAIAN_RESET_ALLOWED_DBS` ersätter listan. Ett namn som matchar `braian-perf-*-stack` nekas alltid. Värden för host måste vara `localhost`, `127.0.0.1`, `::1`, det valda containernamnet, eller en post i `BRAIAN_RESET_ALLOWED_HOSTS` (`host` eller `host:port`).

På demovärden är Postgres containern `braian-postgres-1` (`postgres:18`) och värden saknar klientverktyg. `--container` eller `BRAIAN_PG_CONTAINER` väljer container (det namnet är standard när containern körs). `pg_dump`, sessionskollen, `dropdb` och `createdb` körs med `docker exec`, och dumpen streamas till en fil på värden. Inuti containern ansluter klienten till `127.0.0.1:5432`. Utan container måste lokala `pg_dump`, `psql`, `dropdb` och `createdb` vara major-version 18.

Backupen skrivs till `BRAIAN_BACKUP_DIR` (standard `./backups`) med `umask 077`, så katalogen blir `0700` och filen `0600`. Skriptet avbryter om backupen misslyckas. Seed använder `BRAIAN_BIN`, annars `target/release/braian` om den är körbar, och annars `cargo run -p braian-server -- seed`. `braian seed` kör migrationerna vid anslutning, laddar katalogen och reindexerar när `TYPESENSE_URL` och `TYPESENSE_API_KEY` är satta. Skriptet anropar inte `braian reindex` en gång till.

`braian api` läser `DATABASE_URL`, `BRAIAN_DEV_TOKENS` och `BRAIAN_API_ADDR` (standard `127.0.0.1:8090`). `braian openapi` skriver OpenAPI-dokumentet. `braian all` kör migrationer, demodata, indexer, live-ström och API. Indexern startar när `TYPESENSE_URL` är satt. En gammal `KAFKA_BROKERS` loggas och ignoreras. `just dev` startar dessutom webbgränssnittet på http://127.0.0.1:3000. Demokontot kommer från `BRAIAN_DEMO_EMAIL` och `BRAIAN_DEMO_PASSWORD` i `.env`.

`just spike-1` mäter import och ändringar för ett dokument, en del per språk, och en del per språk och attributgrupp. Standard är tung nivå, 20 000 objekt och 2 000 ändringar per ändringstyp. Argument efter kommandot skickas vidare, till exempel `just spike-1 --objects 300`. Resultatet skrivs till `target/spike-1.json`. Spiken körs inte i CI. Siffrorna är indikativa när Postgres körs i Docker.

Integrationstesterna i `crates/store` kräver `BRAIAN_TEST_DATABASE_URL`. Den ska peka på underhållsdatabasen `postgres`, till exempel `postgres://braian:byt-mig@localhost:5433/postgres`. Varje test skapar en egen databas från en migrerad mall. Saknas variabeln avbryts testet med ett fel.

`just demo-1` behöver `wasm-bindgen` 0.2.129 på `PATH` (`cargo install wasm-bindgen-cli --version 0.2.129`).
Devcontainern installerar samma version. wasm-bindgen's Node-mål är CommonJS, så `packages/rules-wasm` har ingen `"type": "module"`; skripten är `.mjs`.

## Ids, keys and groups

Definitions are addressed by id or key. `GET /v1/resolve/{kind}/{id_or_key}` returns `{id, key, label}`. `POST /v1/model/keys` renames a key immediately; `dry_run=true` returns the warning without writing. The warning is `External integrations that use the old key « x » will stop working`. View state, selection queries and import mappings store ids. A selection's type lives in `assortments.type_id`; the query keeps group and filter. Search fields are `a_<attribute id>` with hyphens replaced by underscores, so an attribute key rename does not rebuild the index. A type, structure or group key rename does, because those keys are still copied into the document. Migration V18 rewrites stored key refs to ids and sets `reindex_requested`. It does not reset demo data. The API field for a legacy class identifier is `group`, and `class` remains an alias for one release. The Classes view is titled Group attributes.

## Ändringslogg

Indexern och SSE läser `changes` direkt. Design, garantier och avvägningar står i
`docs/events.md`. En värd som uppgraderar från relayn måste släppa slotten
`braian_relay` innan Postgres startas om utan `wal_level=logical`.
`deploy/drop-relay-slot.sql` gör det. API:ts `/healthz` säger att processen lever.

## Interaktiv trafik och massinläsning

API:t har två Postgres-pooler. Interaktiva läsningar och skrivningar tar
`BRAIAN_POOL_INTERACTIVE` (standard 24). Massinläsning, bulk-jobb och indexern
tar `BRAIAN_POOL_BULK` (standard 8), så att en COPY inte kan ta alla anslutningar.
Åtta räcker till benchens standard på åtta loaders. Sätt `12` på en värd där
importen ska använda mer av maskinen; det var poolstorleken som återställde
bulk-takten när fyra anslutningar höll igen den.

Varje pågående interaktiv läsning eller skrivning räknas.
Hälsokontroll, OpenAPI, SSE-strömmen och själva `POST /v1/bulk` räknas inte:
en öppen flik skulle annars pausa importen hela tiden. Räkningen i
API-processen är omedelbar för `POST /v1/bulk` i samma process. Den skrivs
också till tabellen `interactive_pressure`, en rad per process, med
tidsstämpel. En massinläsning i en annan process, till exempel
`braian-bench perf-load` som skriver med COPY, läser summan av de färska
raderna innan nästa slice. En rad som inte uppdaterats på 400 ms räknas
inte, så en kraschad process inte lämnar importen pausad.

Innan nästa bulk-slice hämtar en anslutning tittar jobbet på den samlade
räkningen. Är den noll väntar jobbet `BRAIAN_BULK_YIELD_MS` (standard 1,
`0` yieldar bara). Är den över noll växer väntan med
`BRAIAN_BULK_YIELD_STEP_MS` per pågående anrop (standard 4) och stannar vid
`BRAIAN_BULK_YIELD_MAX_MS` (standard 25). Slice:n blir också mindre:
konfigurerad storlek delat med `1 + pågående anrop`, minst en rad och högst
32. Taket gör att en lång klickström saktar bulken men inte parkerar den.

Bulk-slice:ar sätter `synchronous_commit = off` inuti transaktionen. Slice:n
är fortfarande atomär. Jobbets nästa framstegsskrivning är en vanlig commit
och spolar då även den föregående slice:ns WAL. En krasch kan tappa den sista
slice som inte hunnit bli durable, inte halva slice:n.

`BRAIAN_BULK_JOBS` (standard 1) är hur många bulk-jobb som kör, och
`BRAIAN_BULK_QUEUE` (standard 4) hur många som får vänta. Fler får 429.
`BRAIAN_BULK_CHUNK` (standard 25) är raderna per transaktion i `POST /v1/bulk`
när ingen interaktiv trafik pågår. `braian-bench perf-load` läser
`BRAIAN_POOL_BULK` och `BRAIAN_BULK_YIELD_MS`, `BRAIAN_BULK_YIELD_STEP_MS`
och `BRAIAN_BULK_YIELD_MAX_MS` när de är satta, även `0`. Den interaktiva
poolen stannar på kodens standard, så att en export av den inte ändrar en
mätning.

`POST /v1/objects:batchGet` läser upp till 200 id. Utan `values`, `fields`
eller `parts` kommer bara huvudet och grupperna. `fields` är attributnycklar;
`languages` begränsar språkberoende fält till de delarna.

## Typesense och indexern på en 8-kärnig värd

`deploy/compose.perf.yaml` är ett overlay för en värd med 8 vCPU, samma klass
som perf-körningarna. Det ger Typesense två egna kärnor och lastgeneratorn en.
Dev-compose (`just up`) använder det inte.

```
docker compose -f compose.yaml -f deploy/compose.perf.yaml up -d postgres typesense
```

| Kärnor | Process |
| --- | --- |
| 0–2 | Postgres |
| 3 | `taskset -c 3` på `braian api` och `braian indexer` |
| 4 | ledig |
| 5 | lastgeneratorn (`perf`-profilen, eller `taskset -c 5`) |
| 6–7 | Typesense, `--thread-pool-size=2` |

Indexern har två spår. Det interaktiva spåret använder
`BRAIAN_INDEXER_BATCH` (standard 200), `BRAIAN_INDEXER_IMPORT_BATCH`
(standard 100) och `BRAIAN_INDEXER_IMPORT_CONCURRENCY` (standard 4). Det
läser Postgres från `BRAIAN_INDEXER_POOL` (standard 2), skild från
`BRAIAN_POOL_BULK`, och skriver på en egen HTTP-klient. En bulk-import
håller sin anslutning tills Typesense är klar med hela anropet, och den
interaktiva upserten ska inte vänta på den anslutningen eller på bulkens
Postgres-anslutning.

Bulk-spåret läser `BRAIAN_INDEXER_BULK_BATCH` händelser (standard 2 000)
från bulk-poolen, bygger dokumenten och importerar dem i vågor. De första
läsningarna är takade vid 200 händelser tills två fulla batchar är klara,
och växer sedan mot den konfigurerade storleken. En kall läsning av 2 000
händelser höll det interaktiva spåret i ungefär 3 s. Nästa läsning från
loggen och nästa dokumentbygge körs medan den pågående importen är i
Typesense. Claimen för den följande batchen committas först när dess egen
import är klar, så en krasch inte hoppar över den. Varje spår loggar
`fetch`, `build` och `import` som ett tracing-span `indexer` med `lane`,
`events` och `elapsed_ms`.

`BRAIAN_INDEXER_BULK_IMPORT_BATCH` (standard 200, tak 200) och
`BRAIAN_INDEXER_BULK_IMPORT_CONCURRENCY` (standard 2, tak 2) är taket, inte
den våg som alltid skickas. Fyra anrop på 500 dokument höll Typesense så
länge att en enstaka upsert fick vänta ut importen. Före varje våg tittar
spåret på det interaktiva spåret och på `interactive_pressure`:

- Tomt spår och ingen pressure: vågen börjar på 100 × 1 och växer medan
  spåret förblir tomt. Ett andra anrop används bara om det förra anropet
  blev klart inom 50 ms. En köad redigering nollställer vågen till 100 × 1.
- Pressure men tomt spår: ett anrop på den konfigurerade batchen (en
  Typesense-tråd ledig) och högst 15 ms väntan.
- Händelser i spåret, eller en flush som pågår: ett anrop på högst 100
  dokument, och väntan upp till `BRAIAN_INDEXER_BULK_YIELD_MAX_MS`
  (standard 100). Sedan importerar det ändå.

`0` stänger av väntan. Omindexering använder samma vågor. På layouten ovan
har Typesense två trådar (`--thread-pool-size=2`); under pressure och under
en långsam idle-våg lämnas en tråd ledig.

På en maskin med färre kärnor, ändra `cpuset` i overlayet innan det startas.
En kärna som inte finns gör att containern inte startar.

## Postgres-MCP

`.cursor/mcp.json` startar Postgres MCP Pro i läsläge mot utvecklingsdatabasen,
med rollen `braian_readonly` (`DATABASE_URI` i `.env`). Rollen skapas när Postgres-volymen initieras första gången.

## Import: vad AI-mappningen får se

Import skapar alltid ett förslag (draft) som granskas och godkänns; inget skrivs direkt. Mappningen av kolumner kan ta hjälp av en AI-modell (kräver `OPENAI_API_KEY`). `BRAIAN_IMPORT_AI` styr vad som lämnar servern:

- `off`: ingen AI, bara regelbaserad mappning.
- `headers`: bara kolumnrubriker och filnamn skickas, inga cellvärden (mappningen tappar värdebaserade ledtrådar).
- `samples` (standard): rubriker plus några exempelvärden per kolumn.
