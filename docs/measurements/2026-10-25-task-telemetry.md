# Uppgiftsmätning och skrivlatens

Mätt 2026-10-25 på den här maskinen. Före är `main` utan telemetri. Efter är samma träd med uppgiftsmätningen. Siffrorna är en körning per binär, tagna när maskinen var ledig, före och efter direkt efter varandra.

Telemetrin ligger utanför ändringskön. `POST /v1/telemetry` köar i en bounded kanal och släpper batchen när kön är full. Ämnet är `braian.telemetry.ui`. `PATCH /v1/objects`, relayn, indexern och live-utsändningen är oförändrade. `api-latency` och `perf-load` bygger routern via `AppState::new`, som inte startar usage-publiceraren.

- Hårdvara: virtuell Linux (6.12), 4 vCPU, `MemTotal` 15 GiB. Postgres 18, Redpanda och Typesense i Docker från `compose.yaml`. `rustc 1.93.1`.
- Latens: `/tmp/braian-bench-before` och `/tmp/braian-bench-after`, `api-latency --samples 12 --seed 1`, tung nivå, databas `braian_latency`.
- Live: `braian api` mot en ny databas, 12 PATCH mot `/v1/objects/{id}` med `if-match`, tid från anropsstart tills SSE-händelsen. Efter-binären hade `KAFKA_BROKERS` satt, så usage-publiceraren var igång. Första provet är en kall skrivning, omkring 41 ms båda gångerna.
- Sökbarhet: `perf-load` medium, 200 objekt, 2 laddare, chunk 50, 8 sonder, `probe-interval-ms 100`. Före på `braian_search_before` med slot `braian_relay`. Efter på `braian_search_after2` med `BRAIAN_RELAY_SLOT=braian_relay_after`, eftersom en replikeringsslot är kluster-global och inte kan återanvändas från en annan databas.

## PATCH, tungt objekt

| | Före p50 | Efter p50 | Före p95 | Efter p95 |
| --- | ---: | ---: | ---: | ---: |
| HTTP-patch | 17.69 ms | 16.15 ms | 23.40 ms | 22.25 ms |
| Store-patch | 6.05 ms | 6.09 ms | 6.93 ms | 6.89 ms |
| Validering | 0.50 ms | 0.51 ms | 0.84 ms | 0.70 ms |

Store-patchen är densamma. HTTP-patchen är inte långsammare i den här körningen.

## Live, PATCH till SSE

| | p50 | min | max |
| --- | ---: | ---: | ---: |
| Före | 5.49 ms | 4.80 ms | 41.24 ms |
| Efter | 5.61 ms | 4.25 ms | 40.93 ms |

Skillnaden på p50 är 0.12 ms, inom bruset för 12 prov.

## Ändring till sökbar

| | Före | Efter |
| --- | ---: | ---: |
| change→searchable, alla sonder, p50 | 146.9 ms | 149.2 ms |
| p95 | 151.9 ms | 237.9 ms |
| Sonder som tog timeout | 0 av 8 | 0 av 8 |
| Probe-patch p50 | 7.8 ms | 6.7 ms |
| Bulk | 1952 objekt/s | 1968 objekt/s |

p50 skiljer 2.3 ms. p95 efter är ett enda sonder som föll under pågående bulk (n=1, 237.9 ms). Inget sonder tog timeout.
