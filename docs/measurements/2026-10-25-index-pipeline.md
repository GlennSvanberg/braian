# Indexkedja: relay, indexer och tunga importer

Mätt 2026-10-25 på den här maskinen. Före är `c83690b`. Efter är samma arbetsyta som den här filen, byggd med `cargo build --release -p braian-bench -p braian-server` innan filen skrevs. Siffrorna är en körning per binär, inte en serie.

Det här är inte 1M-körningen. Den körningen (8 vCPU, 15 GB, mediumobjekt) ligger utanför den här maskinen: skrivning omkring 1 000 objekt/s, relay omkring 170 händelser/s, indexer 45–50 dokument/s, och en interaktiv ändring som väntade över en minut. Typesense för en miljon mediumdokument ryms inte här. Den här mätningen visar om samma kodvägar blir snabbare på en last maskinen klarar.

- Hårdvara: virtuell Linux (6.12), 4 vCPU Intel Xeon, `MemTotal` 15 GiB, ingen swap. Postgres 18.6, Redpanda och Typesense 30.2 i Docker, samma `compose.yaml` (`max_slot_wal_keep_size=10GB`, `wal_compression=lz4`). `rustc 1.93.1`.
- Binärer: `/tmp/braian-bench-before` och `/tmp/braian-before` från `c83690b`, och motsvarande `-after` från den här koden.

## Pipeline, 8 000 katalogobjekt

`braian seed` med `BRAIAN_SEED_COUNT=8000` mot en ny databas, medan relay och indexer redan lyssnade. Objekten är den lilla demokatalogen (namn, status, några mått), inte tung nivå. När seed återvänt patchades `SYN-000000` via API:t (`namn.sv = INTERACTIVE-MARKER`). Den skrivningen committas efter bulken, så den ligger bakom det relayn inte hunnit publicera.

| | Före | Efter |
| --- | ---: | ---: |
| Seed, 8 000 objekt | 9,8 s | 9,3 s |
| Relay, tid från seedstart tills bulk-topicens high watermark är 8 000 | 44,8 s (178 ändringar/s) | 10,0 s (797 ändringar/s) |
| Slot-lagg när patchen skickades | 21 MB | 2,4 MB |
| Typesense har 8 000 dokument | 57,6 s från seedstart | 10,2 s från seedstart |
| Indexer ikapp efter att seed återvänt | 47,8 s (167 dokument/s) | 0,9 s |
| Patchen syns i Typesense | 34,5 s | 0,24 s |
| `braian reindex` av 8 013 objekt | 4,43 s (1 809 dokument/s) | 0,86 s (9 323 dokument/s) |

Efter körningen var `braian_relay_slot_wal_status` 1 (`reserved`), `braian_relay_slot_invalidated` 0 och `braian_relay_slot_recoveries_total` 0. Slotten blev inte ogiltig. Det fallet är inte kört här: det kräver att WAL slår i `max_slot_wal_keep_size`. Beteendet och inställningarna för delad respektive egen Postgres står i `docs/dev.md`.

Relayn före höll ungefär 180 händelser/s och hade 21 MB okvitterad WAL när patchen committades. Ändringen blev sökbar först efter 34,5 s. Efter höll relayn ungefär 800 händelser/s, alltså i nivå med seedens skrivtakt, och patchen var sökbar på 0,24 s. Indexern låg inte efter skrivningen. Reindex av samma databas gick från 4,4 s till 0,9 s.

## Skrivning

`api-latency` är ett tungt objekt per anrop, 8 prov. Den vägen är oförändrad: HTTP-create p50 43 ms före och 43 ms efter, HTTP-patch p50 59 ms före och 60 ms efter.

`demo-2` lätt, 2 000 objekt, chunk 500: 0,3 s båda gångerna (6 400/s före, 8 000/s efter). En tredjedels sekund räcker inte för att skilja takterna åt.

Tung nivå, där importen höll hela chunkens JSON i minnet:

| Kommando | Före | Efter |
| --- | ---: | ---: |
| `demo-2 --tier heavy --count 600` (chunk 500, två transaktioner) | 48/s, max RSS 4,05 GiB | 77/s, max RSS 1,20 GiB |
| `spike-storage --tier heavy --objects 150 --edits 12 --chunk 50 --parallel 1`, import per språk och grupp | 70/s, p95 748 ms | 107/s, p95 512 ms |
| samma spike, max RSS för hela processen | 1,24 GiB | 403 MiB |
| `spike-storage --tier medium --objects 300 --edits 20 --chunk 100 --parallel 2`, import per språk och grupp | 1 250/s, p95 147 ms, max RSS 295 MiB | 2 094/s, p95 100 ms, max RSS 78 MiB |

Max RSS är GNU `time` (`Maximum resident set size`) för bench-processen. Tung `demo-2` kördes en extra gång enbart för RSS; takten där (48/s och 77/s) stämmer med den första körningen (50/s och 82/s).

## Kommandon

```
braian-bench api-latency --samples 8
braian-bench demo-2 --tier light --count 2000
braian-bench demo-2 --tier heavy --count 600
braian-bench spike-storage --tier medium --objects 300 --edits 20 --parallel 2 --chunk 100
braian-bench spike-storage --tier heavy --objects 150 --edits 12 --parallel 1 --chunk 50
BRAIAN_SEED_COUNT=8000 braian seed
braian reindex
```

Pipeline-raden startade relayn först (den skapar topicen), sedan API och indexer, mot en tom databas och ett eget sökalias.
