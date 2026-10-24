# Braian

Öppen källkod (MIT), self-hosted datahantering för det PIM brukar användas till.
Färdiga gränssnitt som ett traditionellt PIM, och samma åtgärder via API och MCP.

## Kör lokalt

Kräver Docker Desktop (Windows eller macOS) eller Docker i devcontainern, Rust, Node 22 och pnpm 10.

```bash
cp .env.example .env
just up
just dev
```

- Webb: http://127.0.0.1:3000
- API: http://127.0.0.1:8090
- Demokontot skapas från `BRAIAN_DEMO_EMAIL` och `BRAIAN_DEMO_PASSWORD` i `.env` (se `.env.example`). Lösenordet ligger inte i källkoden.
- In-app-agenten använder `OPENAI_API_KEY` och modellen `BRAIAN_AGENT_MODEL` (standard `gpt-6-luna`). Utan nyckel svarar chatten på svenska att ingen modell är kopplad. Förslag, massändring och återställning fungerar ändå.

`just dev` kör migrationer, demodata, API, relay, indexer och Vite. Avsluta med Ctrl+C.

Mer om portar och kommandon: [docs/dev.md](docs/dev.md). Vad som fungerar ikväll: [docs/status.md](docs/status.md).

## Tester

```bash
just check
just test
just demo-4    # relay, kräver Redpanda
just demo-5    # indexer, kräver Typesense
```

Webb: `pnpm --dir apps/web exec tsc --noEmit`, `pnpm --dir apps/web test`, `pnpm --dir apps/web e2e` (API på 8090 och Vite på 3000 ska köra).
