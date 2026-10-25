# Status 2026-10-25

Braian går att starta lokalt och att klicka igenom. Postgres är källan. Relay, indexer, sök, auth, förslag och webbgränssnittet sitter på samma kedja. Demokatalogen som `just dev` visar är bara seed: 13 hjälteobjekt och 3 000 syntetiska bygg- och friluftsprodukter, 3 013 rader, språk sv/en/de, med strukturer, klasser, värdelistor och enheter. Tester skriver i databasen `braian_e2e` och sökaliaset `braian_e2e`, inte i demon.

Skalprovet (5 000 skrivningar i sekunden, 1 miljon och 10 miljoner objekt) är inte gjort. En virtuell tabell mot 100 013 objekt är mätt, se `docs/measurements/2026-10-25-table-100k.md`.

## Så här kör du

```bash
cp .env.example .env
just up
pnpm install
just dev
```

Öppna http://127.0.0.1:3000. API:t lyssnar på http://127.0.0.1:8090. Demokontot skapas från `BRAIAN_DEMO_EMAIL` och `BRAIAN_DEMO_PASSWORD` i `.env` (se `.env.example`). Inget lösenord ligger i källkoden. Playwright läser samma variabler, från miljön eller från `.env`, och har inget lösenord i specen. Agenten vill ha `OPENAI_API_KEY`. Utan nyckel svarar samma Ctrl+K-panel på engelska att ingen modell är kopplad, och resten av programmet fungerar.

På Windows, om `just` inte är installerat: `.\scripts\up.ps1`, `pnpm install`, `.\scripts\dev.ps1`. `just` installeras med `winget install --id Casey.Just` eller `scoop install just`. Rust-bygget behöver CMake och en C++-kompilator eftersom `rdkafka` använder `cmake-build` (Visual Studio Build Tools, workload "Desktop development with C++").

En ren klon på Linux följde README: `cp .env.example .env`, `just up`, `pnpm install`, `just dev`. API:t lyssnade på 8090 och webben på 3000. Sökningen svarade 3 013 objekt ur den befintliga volymen, hjälteobjekten först (`ART-REGEL-45` och vidare). `just` behöver vara installerat. Första `just dev` byggde Rust-koden i klonen. En befintlig replikeringsslot återanvänds, med en varning, och strömmen fortsätter. Windows-anteckningarna står i README.

## Det som fungerar

- Definitioner, validering (samma regler i Rust och wasm), objekt, relationer, enheter och värdelistor.
- Varje skrivning är en batch med aktör och, för en agent, personen agenten agerar för.
- Relay läser `changes` via logisk replikering och publicerar till Redpanda (`interactive` och `bulk`) med en eftersläpningsmätare. Samma test kan peka om `KAFKA_BROKERS` mot Apache Kafka-profilen i `compose.yaml`.
- Indexer, sök med `wait_seq`, och `braian reindex` som byter alias. Ett fält per språk i en Typesense-samling.
- Massändring som jobb (`bulk.write`) med status och fel per rad.
- Inloggning med lösenord (argon2id), cookie-session, roller admin/editor/viewer, och API-token knuten till en person. Ett ooptimerat debugbygge lät Argon2id och blake2 köra utan optimering, och inloggningen tog omkring tio sekunder på en Windows-laptop. Dev-profilen bygger `argon2` och `blake2` med `opt-level = 3`. Hash plus verify av ett lösenord tog 41 ms i den profilen, se `docs/measurements/2026-10-25-login-debug.md`.
- Operationer som UI, Ctrl+K, agenten och `/mcp` delar, inklusive UI-operationer (`ui.open_view`, `ui.open_template`, `ui.open_workspace`, `ui.place`, `ui.link`, `ui.filter`, `ui.group`, `ui.sort`, `ui.select`, `ui.select_rows`, `ui.show_column`, `ui.hide_column`, `ui.move_column`, `ui.set_theme`, `ui.export`). De returnerar en beskrivning som webbläsaren tillämpar. En extern MCP-klient får samma beskrivning och får tillämpa den själv. En agent skriver inte direkt: `create_object`, `patch_object` och `set_relation` blir förslag, även när anropet kommer från den inloggade personen via `/v1/agent`. In-app-agenten anropar OpenAI Responses (`/v1/responses`) med `reasoning.effort` satt till `none`, så `gpt-6-luna` tar emot verktyg. Punkt i ett operationsnamn blir understreck i verktygsnamnet, och det namnet mappar tillbaka till operationen. `POST /v1/agent` med `stream: true` skickar stegen som SSE (`step`, `tool`, `proposal`, `answer`). Utan nyckel är svaret samma engelska mening som förut.
- Arbetsytor och vyer som data, låsbara och delbara. Fyra arbetsytor seedas: Sätt upp systemet (checklista och översikt), Datamodell (träd, klass, konsekvens), Berika objekt (träd, tabell, detalj, graf, historik) och Granska (förslag, batchlogg, jobb). En arbetsyta kan skapas från en mall (högerklick på fliken, eller Ctrl+K). Lås och dela sitter på samma meny. Delningens token kopieras; det finns ingen separat behörighetsvy. Rutorna storleksändras med `react-resizable-panels` och förhållandet sparas när man släpper.
- Tabell med serverfilter, serversortering och servergruppering. Kolumnordning, bredder, filter, gruppering och sortering sparas i vyn och kommer tillbaka vid omladdning. Piltangenter, Page Up/Down, Home och End flyttar markören. Shift utökar urvalet, Ctrl klickar av och på. Enter öppnar. Esc stänger menyn. Ctrl+Alt+pilar flyttar fokus mellan rutor. Samma operationer finns i högerklicksmenyn, med kortkommandot bredvid. Massändring av markerade rader blir ett förslag. En flik kan dras till en droppzon (vänster, höger, över, under, mitten) när arbetsytan inte är låst.
- Detaljformulär i två kolumner. Språkfält är små kort med etiketten sv, en eller de. Tal med enhet och intervall visas som mått. Flervärden som inte är värdelista visas som brickor. En objektreferens visar nummer och namn och väljs i en lista, inte som ett rått id. Piltangenter flyttar mellan fält.
- Datamodell: strukturträd, klass under sin grupp, dra en klass till en annan grupp skriver definitionen, konsekvens av att ta bort ett attribut eller en klass med antal berörda objekt, ett urval och de jobb som deklarerat att de läser fältet.
- Sortiment som snabbfilter (Tält, Virke). Jobbhistorik per jobbtyp, med en seedad körning per typ. Översikt med ett stapeldiagram (TanStack Charts) över antal per typ. Historik med tidslinje, "As it was at #seq", fältdiff och återställning som förslag. Grafen är en SVG-nodkarta: objektet, relationer och var attributet kommer ifrån. Noderna kan dras. Dubbelklick på en relation öppnar det objektet. Layouten är fast, inte en fysiksimulering, och vyn delar den smala detaljrutan.
- OpenAPI beskriver alla rutter, 49 stycken, inklusive plattformens. Filen genereras om med `just openapi`.
- Realtid till webbläsaren (SSE, med ping var 15:e sekund). En ny rad syns i aktivitetsloggen utan omladdning, som en mening (`live · Created …`), inte som rå JSON. Raden eller detaljen blinkar till i accentfärgen.

## Mätt mot 100 013 rader

Tre Playwright-körningar på den här virtuella maskinen (4 vCPU). Första objektraden efter inloggning: 489–512 ms. Filter `Kupol`: 196–199 ms. Scroll över hela höjden 2 400 337 px: 1354–1370 ms, långsammaste frame 27–46 ms. Ett sökuppslag av första sidan tog 19 ms. Siffrorna och kommandona ligger i `docs/measurements/2026-10-25-table-100k.md`.

## Det som är skrovligt

- Ctrl+K är en panel. Grupperna är Recent, Best match, On selected object, Views & workspaces och Objects. Sista raden är alltid "Ask AI: …". Ctrl+J och Agent-knappen öppnar AI-läget i samma panel, med lila kant, tidigare turer, en förloppsrad och stegen på engelska. Modellen instrueras att svara på engelska. Förslag förhandsvisas där med Cancel och Confirm. Det finns inget separat chattfönster. Frecency ligger i webbläsarens localStorage. "Next time: …"-toasten visas när raden har en sparad genväg. En tom detalj visar en ledtext och kortkommandon i stället för bara ordet Detail.
- Grafens layout är handplacerad. I den smala detaljrutan blir kartan trång.
- Konsekvensvyn är texten från `/v1/impact` (berörda objekt, urval, jobb), inte en egen diff av modelländringen innan man bekräftar.
- Entra/OIDC, Excel-import och Tauri är inte byggda. En vytyp kan bädda in en URL, och vytyper kan registreras via API:t. Det finns inget pluginramverk utöver det.
- Skrivtakt, indexeftersläpning under last, 1 miljon och 10 miljoner objekt är inte mätta.

## Kända fel

- Relayn bekräftar slotten inifrån läsloopen. En krasch kan spela upp en kort svans. Indexern är idempotent, men en konsument som inte tål dubbletter får se samma `seq` igen.
- Sökningen är till slut konsekvent utom när anropet skickar `wait_seq` från skrivsvaret.
- Exakt decimal ligger i Postgres. Indexet lagrar basenhetens värde som flyttal, så stora tal kan tappa precision i sortering.

## Tester

`cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (relay- och indexertesten hoppar över om `KAFKA_BROKERS` respektive `TYPESENSE_URL` saknas), wasm-bygget av `rules`, `pnpm --dir apps/web exec tsc --noEmit`, vitest och Playwright. `node scripts/e2e.mjs` kör röken mot `braian_e2e`. `e2e/drag.spec.ts` drar en kolumn, dockar en flik och flyttar en klass. `e2e/latency.spec.ts` är mätningen ovan och hoppas över om inte `BRAIAN_SCALE=1`.
