# Spike 2 – Typesense layout (2026-10-24)

Indicative. Typesense 30.2 in Docker on this Linux VM (`nproc` and `free -h` at the time of the run). Not a customer environment. The script imported 8,000 synthetic documents, three languages (`sv`, `en`, `de`), then read `typesense_memory_active_bytes` from `/metrics.json`.

| Layout | Writes | Import | Objects/s | Full reindex | Memory note |
| --- | ---: | ---: | ---: | ---: | --- |
| One collection, a field per language (`name_sv`, `name_en`, `name_de`) | 8,000 | 0.143 s | 55,925 | 0.143 s | +12.3 MB from a cold collection (`typesense_memory_active_bytes` 56.7 MB → 68.8 MB) |
| One collection per language | 24,000 | 0.325 s | 24,638 | 0.325 s | Could not be isolated. Typesense kept the previous allocation after the first collection was dropped, so the delta (+3.6 MB) is not the cost of the layout. |

## Decision

**One collection, a field per language.**

Import and full reindex were about twice as fast, because language-independent fields are written once. Full reindex time is also the recovery time if Typesense is lost. One document per object keeps the alias swap and read-your-writes on a single sequence number.

The index stores a number's entered unit and the value normalized to the family's base unit (float, for sort and range filters). The exact decimal stays in Postgres.
