# Braian

Öppen källkod (MIT), self-hosted datahantering för det PIM brukar användas till.
Färdiga gränssnitt som ett traditionellt PIM, och samma åtgärder via API och MCP.

## Kör lokalt

Kräver Docker Desktop (Windows eller macOS) eller Docker i devcontainern, Rust, Node 22, pnpm 10 och [just](https://just.systems/).

```bash
cp .env.example .env
just up
pnpm install
just dev
```

- Webb: http://127.0.0.1:3000
- API: http://127.0.0.1:8090
- Demokontot skapas från `BRAIAN_DEMO_EMAIL` och `BRAIAN_DEMO_PASSWORD` i `.env` (se `.env.example`). Lösenordet ligger inte i källkoden.
- In-app-agenten använder `OPENAI_API_KEY` och modellen `BRAIAN_AGENT_MODEL` (standard `gpt-6-luna`). Utan nyckel svarar samma panel på engelska att ingen modell är kopplad. Förslag, massändring och återställning fungerar ändå. All gränssnittstext är engelska; katalogvärden i demot ligger kvar på sv/en/de.

`pnpm install` behövs en gång, innan Vite kan starta. `just dev` kör migrationer, demodata, API, relay, indexer och Vite, och bygger Rust-koden första gången. Avsluta med Ctrl+C. Demokatalogen är syntetisk: hjälteobjekt plus ett par tusen bygg- och friluftsartiklar (`BRAIAN_SEED_COUNT`, standard 3000) på svenska, engelska och tyska. Tester skriver inte i den databasen.

## Windows och Docker Desktop

Kör Docker Desktop. Snabbast går det i WSL2 med repot i Linux-filsystemet (`~/src`), inte under `/mnt/c`. Rust, Node 22 och pnpm 10 ska ligga i samma skal som startkommandot.

`just` finns inte i en vanlig Windows-installation. Installera det med `winget install --id Casey.Just` eller `scoop install just`. Justfilen anropar PowerShell (`windows-shell`). Utan `just` är motsvarigheterna:

```powershell
copy .env.example .env
.\scripts\up.ps1
pnpm install
.\scripts\dev.ps1
```

`scripts/dev.ps1` läser `.env` (värden med citattecken, till exempel `BRAIAN_DEV_TOKENS`, behåller semikolon) och startar sedan samma processer som `just dev`.

Rust-bygget kompilerar `rdkafka` med feature `cmake-build`, så CMake och en C++-kompilator behövs. Visual Studio Build Tools med workloaden "Desktop development with C++" ger båda. CMake kan också installeras med `winget install --id Kitware.CMake`, men kompilatorn från Build Tools behövs ändå. Samma feature används i CI, som bygger `librdkafka` med CMake. `dynamic-linking` skulle i stället kräva en systeminstallerad `librdkafka`.

- Portar på värden: Postgres 5433, Redpanda 19092, Typesense 8108, Redpanda Console 8080, API 8090, webb 3000. API:t ligger på 8090 för att konsolen redan tar 8080.
- Lämna `BRAIAN_COOKIE_SECURE=0` lokalt. Sätt den till 1 bara bakom HTTPS.
- Playwright-röken (`node scripts/e2e.mjs`) är körd på Linux. Den skapar databasen `braian_e2e` och ett eget sökalias, utan Kafka, så `just dev` kan stå kvar. På Windows startar skriptet `pnpm.cmd` med `shell: true`, så Node hittar pnpm.

Mer om portar och kommandon: [docs/dev.md](docs/dev.md). Vad som fungerar ikväll: [docs/status.md](docs/status.md).

## Tester

```bash
just check
just test
just demo-4    # relay, kräver Redpanda
just demo-5    # indexer, kräver Typesense
```

Webb: `pnpm --dir apps/web exec tsc --noEmit` och `pnpm --dir apps/web test`. `node scripts/e2e.mjs` startar API och Vite mot databasen `braian_e2e` (inte `braian`) och kör Playwright. `just dev` behöver inte vara igång, men `just up` ska ha startat Postgres och Typesense.
