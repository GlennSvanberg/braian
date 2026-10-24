# OpenPIM (arbetsnamn)

Öppen källkod (MIT), self-hosted datahanteringssystem för det PIM brukar användas till.
Positionering: mellan Akeneo och Supabase. Färdiga gränssnitt som ett traditionellt PIM,
men allt nåbart via API/MCP. Inte Fiwes produkt – Fiwe använder det.

## Källa för designen
- Tes och designprinciper (utkast): https://app.notion.com/p/403212bf181a81b4b96ed5d5dd0d77a7
  Läs sidan innan större beslut. Allt där är diskussion och riktningar, inte strikta krav.

## Nuvarande fokus: Steg 1 – grunden, tunn kedja hela vägen
1. Definitioner: typ, struktur med klasser, attribut med regler.
2. Skriva: API validerar, sparar i Postgres, outbox -> Redpanda.
3. Indexera: indexerare läser Redpanda, uppdaterar Typesense inom sekunder.
4. Läsa: sök/lista via Typesense, enskilt objekt från Postgres.
5. Skalbevis: 1 miljon syntetiska objekt; mät skrivtakt, fördröjning till index, söklatens.
Agent, UI, realtid och webhooks kommer sedan som konsumenter av samma lager.

## Riktningar (2026-10-24)
- Enterprise-skala (miljontals objekt). Self-hosting får vara flera komponenter.
- Redpanda är kärnan från start. Kort fördröjning admin -> index (sekunder, inte minuter).
- Ingen värdearv; hellre delade värden. Språk på värdenivå. JSONB per objekt, inte EAV.
- Objekt = typdefinition + klasser från sin strukturgrupp (en klass hör till en strukturgrupp).
  Inga låsta typer som artikel/produkt.
- Intern validering alltid på i API:t (typ, längd, värdelista, enhet). Extern DQ async via loggen.
- Varje sparning är en batch internt; användaren ska inte märka tekniken.
- Versionerade definitioner, snygg migrering av brytande ändringar.
- Audit trail i Postgres, rollback som nya append-only-batcher.
- Människa och agent behandlas lika; agent agerar alltid för en person.
- Testdata: först enbart syntetisk. Aldrig kunddata.

## Regler för agenter
- Inga hemligheter i repo, loggar eller chatt. Använd en gitignorerad .env.
- Mät, gissa inte: prestandapåståenden ska ha siffror.


## Repo
- Cargo-workspace i `crates/`, pnpm-workspace för `apps/` och `packages/`.
- `model` och `rules` får inte bero på I/O; `rules` måste kompilera till `wasm32-unknown-unknown`.
- En binär `openpim` med roller (`api`, `relay`, `indexer`, `all`) i `crates/server`.
- Kommandon via `just` (se `docs/dev.md`). Mätresultat i `docs/measurements/`.
- Designkällan är Notion-sidan; repot dokumenterar bara det som hör ihop med koden.

## Teknikval (2026-10-24)
- Kärna och backend (API, indexerare, validering): Rust.
- Frontend: TypeScript med TanStack Start. Frontend är första klienten mot API:t (bevisar API-first).
- Snabbhet i UI via lokal data, optimistiska uppdateringar och realtidspush, inte via WASM-ramverk.
- Valideringsreglerna kan kompileras från Rust till WebAssembly så samma regler körs i klient och API.
- Infrastruktur: Postgres, Redpanda, Typesense (docker-compose för utveckling).
