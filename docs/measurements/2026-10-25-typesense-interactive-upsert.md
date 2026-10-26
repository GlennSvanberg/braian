# Typesense: en upsert medan en bulkimport pågår

Historik från 2026-10-25. Siffrorna gäller Typesense, inte en broker. Nuvarande flöde står i `docs/events.md`.

Mätt 2026-10-25. Det här är inte perf-load och inte hela indexkedjan. Det mäter bara det indexern fastnade i: en enstaka `documents` upsert som skickas medan `documents/import` redan pågår. Siffrorna avgör standardstorleken på bulk-vågen.

- Hårdvara: virtuell Linux, 4 vCPU Intel Xeon, `MemTotal` 15 GiB. Typesense 30.2 som egen process, inte Docker, `--thread-pool-size=2` (samma tak som `deploy/compose.perf.yaml`), `127.0.0.1:8108`.
- Dokument: 24 strängfält, JSON på 55 968 byte per dokument. Import och upsert är `action=upsert`. Upserten öppnar en egen HTTP-anslutning, så kön sitter i Typesense och inte i klientens pool.
- Varje rad är 8 försök utom "ensam upsert" som är 10. p95 på 8 värden är det näst högsta.

| Samtidigt | Upsert p50 | Upsert p95 | Import p50 |
| --- | ---: | ---: | ---: |
| ingen import | 5 ms | 6 ms | – |
| 4 × 500 dokument | 137 ms | 197 ms | 2 418 ms |
| 1 × 500 dokument | 71 ms | 100 ms | 862 ms |
| 2 × 200 dokument | 52 ms | 65 ms | 521 ms |
| 1 × 150 dokument | 35 ms | 60 ms | 263 ms |
| 1 × 100 dokument | 26 ms | 49 ms | 182 ms |

En egen anslutning räcker inte. 4 × 500 har redan en egen anslutning för upserten och ligger ändå på 197 ms p95, medan importen själv tar 2–3,5 s. Med en samtidig import på 100 dokument är upserten 49 ms p95 trots att importen tar ~180 ms: Typesense har en tråd ledig (`thread-pool-size=2`, samtidighet 1).

Mätningen motiverade taket 200 dokument och samtidighet 2, och att en upptagen våg stannar på ett anrop. Standard vid mättillfället var 100 × 1. Arkitekturgranskningen samma dag höjde den lediga standarden till 200 × 2 och lät pressure använda 200 × 1; en köad interaktiv händelse är fortfarande högst 100 × 1. En tidigare körning med lättare dokument (12 fält) gav samma ordning: 4 × 500 upsert p95 117 ms, 1 × 150 p95 19 ms.

Hela ändring→sökbar-kedjan på 50 000 objekt är inte körd här. Den körningen gjordes på den pinnade 8-vCPU-layouten, med den broker som sedan togs bort.
