# Spike 1: how object values are stored

Indicative numbers from one Windows machine, with Postgres in Docker. They compare three ways to split the same `object_values` rows. They are not a proof that 5,000 heavy objects per second is reachable on this hardware.

- Hardware: Intel Core Ultra 7 155U (12 cores, 14 threads), 32 GB RAM, KIOXIA KXG8AZNV1T02 NVMe 1 TB, Windows 11. The bench process ran on the host. Postgres 18.6 ran in Docker Desktop. `rustc 1.93.1`.
- Postgres: `wal_compression=lz4`, `default_toast_compression=lz4`, `synchronous_commit=on`, `max_wal_size=8GB`.
- Data: heavy tier, seed 1. 2,000 attributes plus one unused attribute (2,001), 20 languages, 60% of attributes filled. Text values are 32 ASCII characters, so real product text will be larger. Phase 1 measured one heavy object at about 263 KB of JSON.
- Command: `cargo run -p braian-bench --release -- spike-storage --parallel 2`. The default worker count is the logical processor count. Fourteen workers each holding a chunk of 500 heavy objects committed about 47 GB and the machine swapped, so this run used two workers. All three forms used that same setting.
- Import: 20,000 objects, binary `COPY`, 500 objects per transaction. Edits: 2,000 per workload, on objects that were not edited by another workload. Each workload runs `VACUUM` and `CHECKPOINT` first, then resets `pg_stat_wal`.
- Rate: objects divided by the slowest worker's summed write time. Object generation is outside that time. With two workers running together, that time is close to the wall clock. Percentiles are per transaction. An import or chunked transaction holds up to 500 objects. An interactive transaction holds one.
- WAL bytes are `pg_wal_lsn_diff` of `pg_current_wal_insert_lsn` across the workload. `pg_stat_wal.wal_bytes` stayed within a few percent of that difference. Full-page images are `pg_stat_wal.wal_fpi`. The checkpoint at the start of each workload makes the next dirty pages write a full image, and every form got the same treatment.
- Validation is not in these times. Phase 1 measured about 1.5 ms per heavy object on one core, so 5,000 per second needs about eight cores for validation alone.

The three forms share one schema. Only the function that picks the part name changes.

- **document:** every value in part `""`, language maps left inside the value.
- **per-language:** language-independent values in `""`, and one part per language tag. The language wrapper is lifted out of the value.
- **per-language-group:** the same split, then attributes sorted by key in blocks of 100. Heavy definitions have 21 blocks, not 20, because of the unused attribute. This import wrote 2,680,000 value rows for 20,000 objects, 134 parts per object on average.

## Import

| Form | Objects/s | WAL bytes/object | Full-page images |
| --- | ---: | ---: | ---: |
| document | 11.9 | 139 294 | 2 225 |
| per-language | 17.1 | 214 116 | 8 207 |
| per-language-group | 29.6 | 242 589 | 12 636 |

## Edits

WAL bytes per change:

| Workload | document | per-language | per-language-group |
| --- | ---: | ---: | ---: |
| Language-independent, one transaction per object | 136 868 | 36 421 | 12 075 |
| Language-independent, chunks of 500 | 138 930 | 38 104 | 12 355 |
| One language, one transaction per object | 139 749 | 23 463 | 11 350 |
| One language, chunks of 500 | 139 608 | 24 747 | 11 379 |
| Five mixed attributes, one transaction per object | 138 455 | 60 845 | 21 280 |
| Five mixed attributes, chunks of 500 | 137 825 | 76 216 | 31 753 |

Objects per second, same workloads:

| Workload | document | per-language | per-language-group |
| --- | ---: | ---: | ---: |
| Language-independent, one transaction per object | 19.6 | 31.2 | 97.4 |
| Language-independent, chunks of 500 | 25.2 | 36.1 | 179.2 |
| One language, one transaction per object | 21.0 | 32.1 | 97.3 |
| One language, chunks of 500 | 22.5 | 38.3 | 161.4 |
| Five mixed attributes, one transaction per object | 8.4 | 18.1 | 86.0 |
| Five mixed attributes, chunks of 500 | 20.2 | 19.4 | 142.6 |

Interactive p95 on the grouped form was about 30 ms per object. A document edit rewrites the whole value, so its WAL stays near the import cost no matter how small the patch is.

## Disk after the import

`object_values` plus the `changes` rows for the creates. Heap, TOAST and indexes are included in the total.

| Form | `object_values` | Per object | `changes` |
| --- | ---: | ---: | ---: |
| document | 1.34 GB | 66 852 B | 1.36 GB |
| per-language | 2.84 GB | 141 923 B | 1.36 GB |
| per-language-group | 3.62 GB | 180 835 B | 1.36 GB |

The document and per-language values sit in TOAST and compress with lz4. Grouped parts are about 1.3 KB each, under the TOAST threshold, so they stay in the heap and the column compression does not apply. That is why the grouped table is the largest even though each update touches less of it. The `changes` table is the same size for every form: a create records the whole value map, not the parts.

## What 5,000 heavy objects per second would write

WAL per second if this byte cost holds, from the LSN column above.

| Workload | document | per-language | per-language-group |
| --- | ---: | ---: | ---: |
| Import | 696 MB/s | 1 071 MB/s | 1 213 MB/s |
| One language, interactive | 699 MB/s | 117 MB/s | 57 MB/s |
| Five mixed attributes, interactive | 692 MB/s | 304 MB/s | 106 MB/s |

On this machine, with two workers, the grouped form imported at 30 objects per second and applied a one-language edit at 97 per second (161 per second in chunks of 500). That is about 170× short of 5,000 imports per second and about 30× short of 5,000 chunked one-language edits per second. A host that can sustain the WAL rates in the table, with cores left over for the phase 1 validation cost, is what phase 6 has to size.

## Decision

Store values per language and attribute group.

Edits are the workload that has to stay small: a one-language change was 11 KB of WAL instead of 140 KB, and it was the fastest form on every edit. Import was the fastest as well (30/s against 12/s for one document), so the import rate does not argue against it. The price is import WAL and disk. A million heavy objects, at these sizes, is about 181 GB of `object_values` plus about 68 GB of create diffs in `changes`, against about 67 GB plus 68 GB for one document.
