# API latency against a direct store call

Indicative numbers from one Windows machine, with Postgres in Docker. They compare creating one heavy object, and patching three attributes, over loopback HTTP and by calling the store in the same process. They are not a proof of the phase 6 targets.

- Hardware: Intel Core Ultra 7 155U (12 cores, 14 threads), 32 GB RAM, KIOXIA KXG8AZNV1T02 NVMe, Windows 11. The bench process ran on the host. Postgres ran in Docker Desktop. `rustc 1.93.1`.
- Command: `cargo run -p openpim-bench --release -- api-latency --samples 40`. Database `openpim_latency`, heavy tier, seed 1. Three warmup creates ran first and are not in the table.
- HTTP is one keep-alive connection to `127.0.0.1` on a free port. The API validates every write. The direct store call does not. A separate row times validation alone, on one heavy object, in the bench process.
- Each sample is one object. Creates use a fresh object. Patches take the first three attributes of the synthetic mixed edit and apply them to an object that was inserted beforehand and not patched by another sample. The HTTP creates ran before the direct creates, and the HTTP patches before the direct patches, so the later samples see a larger table.
- Percentile: sort the 40 durations and take index `ceil(p * 39)`. With 40 samples, p99 is only the slow end of this run.

A heavy object is about 263 KB of JSON (see `2026-10-24-object-sizes.md`). The create times are the Postgres write. The patch writes one small part.

| Operation | p50 | p95 | p99 |
| --- | ---: | ---: | ---: |
| Create, HTTP | 276.25 ms | 350.69 ms | 373.86 ms |
| Create, store | 279.66 ms | 457.12 ms | 463.58 ms |
| Patch three attributes, HTTP | 44.48 ms | 71.70 ms | 76.59 ms |
| Patch three attributes, store | 18.36 ms | 32.68 ms | 33.50 ms |
| Validate one heavy object | 1.07 ms | 2.40 ms | 2.51 ms |

On a create, the HTTP path and the direct write land on the same p50. The extra work in the API, including validation at about 1 ms, is small next to writing the object. On a three-attribute patch the HTTP path is slower: 44 ms against 18 ms at p50. Validation does not account for that gap. The direct patch samples ran after the HTTP ones, so some of the create p95 gap can be the table growing rather than the call path.
