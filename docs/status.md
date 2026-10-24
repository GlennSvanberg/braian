# Status 2026-10-24

Braian går att starta lokalt och att klicka igenom. Postgres är källan. Relay, indexer, sök, auth, förslag och webbgränssnittet sitter på samma kedja. Skalprovet (5 000 skrivningar i sekunden, 1 miljon och 10 miljoner objekt) är medvetet inte gjort.

## Så här kör du

```bash
cp .env.example .env
just up
just dev
```

Öppna http://127.0.0.1:3000. API:t lyssnar på http://127.0.0.1:8090. Demokontot skapas från `BRAIAN_DEMO_EMAIL` och `BRAIAN_DEMO_PASSWORD` i `.env` (se `.env.example`). Inget lösenord ligger i källkoden. Agenten vill ha `OPENAI_API_KEY`. Utan nyckel säger chatten till på svenska och resten av programmet fungerar.

## Det som fungerar

- Definitioner, validering (samma regler i Rust och wasm), objekt, relationer, enheter och värdelistor.
- Varje skrivning är en batch med aktör och, för en agent, personen agenten agerar för.
- Relay läser `changes` via logisk replikering och publicerar till Redpanda (`interactive` och `bulk`) med en eftersläpningsmätare. Samma test kan peka om `KAFKA_BROKERS` mot Apache Kafka-profilen i `compose.yaml`.
- Indexer, sök med `wait_seq`, och `braian reindex` som byter alias. Ett fält per språk i en Typesense-samling (spike 2).
- Massändring som jobb (`bulk.write`) med status och fel per rad.
- Inloggning med lösenord (argon2id), cookie-session, roller admin/editor/viewer, och API-token knuten till en person.
- Operationer som UI, Ctrl+K, agenten och `/mcp` delar. En agent skriver inte direkt: anropet blir ett förslag.
- Arbetsytor och vyer som data, låsbara och delbara. Fyra arbetsytor seedas: Sätt upp systemet, Datamodell, Berika objekt, Granska.
- Sortiment (sparad sökfråga), jobbtyp, webbhooks, Excel-export, påverkan vid modelländring, historik, objekt som det såg ut vid ett sekvensnummer, och återställning som ett nytt förslag.
- Realtid till webbläsaren (SSE). En ny rad syns i aktivitetsloggen utan omladdning.
- Webb: mörkt och ljust, arbetsytor, delade rutor, träd, virtuell tabell, detaljformulär, graf, historik, förslag, kommandopalett, agentpanel som degraderar utan nyckel.
- Syntetisk katalog med friluft och bygg, språk sv/en/de.

## Det som är skrovligt

- OpenAPI beskriver inte de nya plattformsroutterna (auth, förslag, arbetsytor, sök). Kontraktstestet jämför bara det genererade dokumentet med filen.
- Grafvyn är en enkel SVG, inte en riktig nodkarta.
- Tabellens kolumnval, bredder och filter sparas inte tillbaka till vyn ännu. Arbetsytans layout sparas om den inte är låst.
- Dra-och-släpp av klass till grupp i datamodellen ritar klassen men skriver inte om definitionen.
- Aktivitetsloggen visar rå JSON från strömmen.
- Demokatalogen är liten. Seed hoppar över objekt som redan finns, så en redigerad demo-nyckel skrivs inte över.
- Entra/OIDC och Tauri är inte byggda. Pluginramverk heller inte: en vytyp kan bädda in en URL, och vytyper kan registreras via API:t.

## Kända fel

- Relayn bekräftar slotten inifrån läsloopen. En krasch kan spela upp en kort svans. Indexern är idempotent, men en konsument som inte tål dubbletter får se samma `seq` igen.
- Indexern dog tidigare om bulk-ämnet inte hade någon offset att committa. Det är rättat. Starta om `braian all` om en gammal process fortfarande loggar `indexer stopped`.
- Sökningen är till slut konsekvent utom när anropet skickar `wait_seq` från skrivsvaret.
- Exakt decimal ligger i Postgres. Indexet lagrar basenhetens värde som flyttal, så stora tal kan tappa precision i sortering.

## Tester

`cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (relay- och indexertesten hoppar över om `KAFKA_BROKERS` respektive `TYPESENSE_URL` saknas), wasm-bygget av `rules`, `pnpm --dir apps/web exec tsc --noEmit`, vitest och Playwright (`apps/web/e2e/smoke.spec.ts`: inloggning, skapa, valideringsfel, spara, Ctrl+K, förslag, återställning, live-ström).

## Nästa steg

- Fyll OpenAPI för plattformsroutterna.
- Låt tabellens kolumner och filter sparas i vyn.
- Dra en klass till en grupp på riktigt, och visa påverkan innan definitionen sparas.
- Koppla Entra. Bygg Tauri-skalet när webben är stabil.
- Mät skrivtakt, indexeftersläpning och söklatens. Skalprovet hör hemma där, med siffror i `docs/measurements/`.
