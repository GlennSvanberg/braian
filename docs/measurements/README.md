# Mätningar

Filer skrivna före 2026-10-26 mäter kedjan med relay och Redpanda. Den kedjan är borta. Nuvarande flöde står i `docs/events.md`. Siffrorna i de äldre filerna är historik och ska inte läsas som nuläget.

En fil per mätning: `YYYY-MM-DD-kort-namn.md`. Varje fil anger:

- git-commit som mättes
- hårdvara (CPU, RAM, disk) och om tjänsterna körde i Docker
- datamängd (antal objekt, storleksnivå, språk)
- hur mätningen kördes (kommando)
- resultat i siffror: takt, p50/p95/p99, resursanvändning
- slutsats och vad den avgör
