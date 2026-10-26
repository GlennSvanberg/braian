# Architecture review, 2026-10-25

This review describes the broker pipeline as it was on that date. The broker, the relay, and the replication slot were removed afterwards. The current flow is [docs/events.md](../events.md). The findings below are kept as the record of that review.

Reviewed from `main` at `03419039` (naming-only change after the v7 indexer). The numbers below are from the v6 and v7 reports on the same 8-vCPU VPS, plus the Typesense microbench and one local document measurement. This branch does not rerun the 50k pipeline. Throughput after the change is an estimate.

v7 is `486047e` (“Keep interactive search updates ahead of bulk Typesense imports”). v6 is `f90f311`. Both use synth medium: 301 attributes, 10 languages, 10 searchable, 12 loaders, chunk 250, isolated Postgres / Redpanda / Typesense.

## Write path

One save is a batch. The API validates it, writes Postgres, and the same `changes` table is the audit trail and the outbox (monthly partitions, `changes_object_seq` on `(object_id, seq)`). The relay tails that table with a logical replication slot and publishes a small `ChangeEvent` — “object X changed”, not the values — to `braian.changes.interactive` or `braian.changes.bulk`.

The producer uses `acks=all`, `enable.idempotence`, `linger.ms=5`, and lz4. Each topic has one partition and replication factor 1, so order on a topic survives a consumer crash and a replay can deliver the same `seq` twice.

The indexer runs two consumer groups. It coalesces by object id (highest `seq`), reads the current Postgres row, builds a Typesense document, imports it, and commits the Kafka offset only after that import succeeds. A crash redelivers.

Search and list read Typesense. One object is read from Postgres. SSE is a second consumer of both topics, plus a local emit in the API process after commit. Bulk SSE frames are folded. The broadcast channel holds 1024 messages; a slow subscriber misses frames. The row is still in Postgres and the event is still in Redpanda.

Interactive commits also call `index_after_commit`. That queues `index_written` on a separate Typesense HTTP client (`interactive_writer`) and does not move the indexer watermark. The queue is 64 deep. Overflow is dropped; the Redpanda consumer repairs it. That write-through is why an edit can become searchable while a bulk import is still running.

## Lanes

Interactive and bulk are separate from the API pool through to Typesense.

- Postgres: interactive pool default 24 (`BRAIAN_POOL_INTERACTIVE`). Bulk COPY, bulk jobs, and the indexer use `BRAIAN_POOL_BULK`, default 8. v7 pool 12 is the faster bulk setting on this host (4118 obj/s Postgres-only vs 3825 at pool 8).
- Redpanda: one topic per lane, so a bulk backlog does not sit in front of an edit on the log.
- Indexer: the interactive lane uses its own batch settings (200 events, import batch 100, concurrency 4) and its own HTTP client. A bulk `documents/import` holds its connection until Typesense finishes, so the upsert must not share that connection.
- Typesense itself is one process. The lanes only stay independent while a thread is free. `deploy/compose.perf.yaml` sets `--thread-pool-size=2`.

## Backpressure

Three layers, and they are not the same knob.

1. Bulk COPY yield. Before the next slice the loader reads `interactive_pressure` (in-process counter, and the Postgres table of the same name so another process can see it). Rows older than 400 ms are ignored. Health, OpenAPI, SSE, and `POST /v1/bulk` itself do not count. Under pressure the slice shrinks to at most 32 rows and pauses `BRAIAN_BULK_YIELD_MS` (default 1) plus `BRAIAN_BULK_YIELD_STEP_MS` (default 4) per in-flight request, capped at `BRAIAN_BULK_YIELD_MAX_MS` (default 25). `POST /v1/bulk` also returns 429 when `BRAIAN_BULK_JOBS` / `BRAIAN_BULK_QUEUE` are full (defaults 1 and 4).
2. Indexer yield. v7 paused up to 100 ms (`BRAIAN_INDEXER_BULK_YIELD_MAX_MS`) and shrank every wave to 100×1 whenever the interactive drain was busy **or** `interactive_pressure > 0`. On the VPS the UI client kept fresh pressure on 71–77% of 200 ms samples (peak in-flight 3). The pause fired on almost every wave, including waves where the interactive topic was empty.
3. Typesense has no preemption. A wave that has already started runs to completion. v6’s 4×500 imports (p50 2.4 s on the microbench) are why edit→searchable p95 went to 1245 ms. Yield cannot cancel them.

v6 did not exercise (1) for perf-load: the counter was in-process, and perf-load is not the API process. v7 fixed that with the pressure table. perf-load still ignored `BRAIAN_BULK_YIELD_*` and only read `BRAIAN_POOL_BULK`. This branch makes it read the three yield variables, including `0`. The interactive pool stays on the code default so a shell export of `BRAIAN_POOL_INTERACTIVE` does not change a run.

## Typesense

One collection. A field per language for attributes that are filterable or sortable (`a_{key}` or `a_{key}__{lang}`), plus `search_{lang}` blobs, `key`, `type_key`, `seq`, `version`, `deleted`, and `groups`. Filterable fields are facets. Sortable fields are sortable. The index is in memory and is not sharded.

The medium document the bench actually indexes, measured locally (seed 1, object 0, searchable cap 10, filterable and sortable uncapped — the same budget the bench uses unless a script passes other caps):

| | count |
| --- | ---: |
| Searchable attributes | 10 |
| Filterable | 100 |
| Sortable | 40 |
| Own schema fields (filter or sort, per language) | 518 |
| Document keys | 312 |
| Document JSON | 9 751 bytes |
| Searchable-only fields omitted by this branch | 50 |

The microbench’s 24-field, 55 968-byte document is a heavier stand-in, not this document. On the live schema, field count dominates byte size. List and palette reads use fields that stay (`namn` is sortable, `status` and `nummer` are filterable). Detail views load `/v1/objects/{id}` from Postgres. Searchable-only text, including demo `beskrivning`, is still copied into `search_{lang}`.

Import is `action=upsert` at the HTTP layer, but `Typesense::import` looks up stored seqs first (chunks of 80), creates new ids, and replaces an existing id only when the incoming seq is newer (`replace_if_newer`: delete where seq is older, then create). Older seqs are skipped.

Microbench, 4 vCPU, Typesense 30.2, `--thread-pool-size=2`, 8 tries (upsert p95 is the second highest). Source: `docs/measurements/2026-10-25-typesense-interactive-upsert.md`.

| In flight | Upsert p50 | Upsert p95 | Import p50 |
| --- | ---: | ---: | ---: |
| none | 5 ms | 6 ms | — |
| 1 × 100 | 26 ms | 49 ms | 182 ms |
| 1 × 150 | 35 ms | 60 ms | 263 ms |
| 2 × 200 | 52 ms | 65 ms | 521 ms |
| 1 × 500 | 71 ms | 100 ms | 862 ms |
| 4 × 500 | 137 ms | 197 ms | 2 418 ms |

Single-thread imports plateau around 550–580 docs/s on that payload (100/0.182 s ≈ 549; 500/0.862 s ≈ 580). Concurrency is what passes 600: 2×200 is 400/0.521 s ≈ 768 docs/s, with upsert p95 65 ms. 4×500 is the v6 shape: the upsert waits out a multi-second import.

v7’s code default was 100×1, hard caps 200 and 2. With the caps, idle and busy were the same size; the only idle difference was skipping the 100 ms wait. The pinned v7 run set concurrency 2, so idle there could be 100×2. This branch’s default is 200×2 inside the same caps, and the wave depends on why the lane is busy (see Proposals).

## Postgres

`compose.yaml` sets `wal_level=logical`, `max_slot_wal_keep_size=10GB`, `max_wal_size=8GB`, `wal_compression=lz4`. It does not set `synchronous_commit`, so interactive commits use `on`.

Bulk slices run `SET LOCAL synchronous_commit = off`. The slice is still atomic. The next ordinary commit flushes the previous slice’s WAL. A crash can drop the last unflushed slice, not half of one. That is a lost write of data that had not been made durable, not a lost event after a committed interactive save.

Hot indexes for this path: `objects_type_id_key_active` (unique on live keys), `objects_active_id`, `objects_active_type_id`, `objects_active_key_trgm`, `object_structures_group`, `object_values_latest`, `changes_object_seq`. The indexer reads the current object by id after the event; it does not trust the event body.

v7 pool 8 vs pool 12 is a Postgres connection effect, not an indexer effect: Postgres-only 10k median 3825 vs 4118 obj/s, while indexer docs/s stayed ~380 (387 and 377). The write path is not what makes 50k objects take ~130 s to become searchable.

## Lost events, duplicates, idempotency

No path treats Redpanda as the source of truth. Postgres is. Typesense and the log are rebuildable: slot invalidation (`wal_status = lost` or any invalidation reason) drops the slot, asks for a full reindex, and creates a new slot. `max_slot_wal_keep_size=10GB` is the bound. A relay or indexer that falls behind a large load can invalidate the slot; the repair is a reindex, not a silent skip.

What can still be lost:

- Replication factor is 1. If the broker loses an acked message after the relay has confirmed the slot, that event is not replayed from Postgres. A later edit of the same object publishes a new seq and repairs the index. An object that never changes again stays stale until a reindex. This is the gap to close before treating the log as durable for slow jobs.
- The last bulk slice with `synchronous_commit=off`, if the process dies before the next ordinary commit.
- SSE and webhook frames, and a write-through queue that overflows. The committed row and the Redpanda event remain.

Duplicates are expected and handled:

- Producer idempotence covers retries inside one producer session. A crash still redelivers, because the offset is committed only after a successful import.
- The indexer keeps the highest seq per object in a batch.
- Typesense refuses a seq that is not newer than the stored one.
- SSE remembers recent seqs and drops a replay of the same seq.

## Scaling

The design goal that is already met on this host: interactive search visibility during bulk. The Notion target discussed for that is 5 s p95. v7 measured 375 ms (pool 8), 258 ms (pool 12), and 140 ms pinned. v6 was 1245 / 1360 / 909 ms on the same three shapes.

The design goal that is not met: sustained bulk around 5000 heavy objects/s into the index. Postgres-only is 3825–4118 obj/s unpinned. The index is ~380 docs/s unpinned and 477 pinned. Fifty thousand objects take about 130 s to be fully searchable (v6 was about 80 s at 622 docs/s, v5 about 105 s at 476).

What does not scale out by adding a process:

- One partition per topic means one indexer consumer per lane. A second consumer in the same group sits idle.
- Typesense is one in-memory index. More indexer concurrency without a free thread recreates the v6 regression.
- Pinned layout (v7): Postgres on cores 0–2, Redpanda plus API, relay, and indexer on core 3, load generator and UI client on 4–5, Typesense on 6–7 with two threads. Bulk falls to 2240 obj/s because the loader has two cores. Edit→searchable p95 falls to 140 ms. Unpinned, Typesense shares CPUs with COPY, and the same p95 is 375 ms.

Adding a slow background job is a new consumer group on the existing topics. It does not require replacing Redpanda. It does require the log to still be there, which is the replication-factor gap above, and it must not import into Typesense on the interactive thread.

## Ranked bottlenecks

1. **Typesense import under the v7 wave policy.** Indexer throughput fell from 622 docs/s (v6) to 387 (v7 pool 8), 377 (pool 12), and 477 (pinned, concurrency 2). Fifty thousand documents at 387 docs/s is 129 s, which is 258 ms per 100 documents. The microbench’s 1×100 import is 182 ms. If pressure is on ~75% of waves, the average 100 ms cap costs ~75 ms. 182 + 75 = 257 ms, and 100/0.257 s ≈ 389 docs/s. That matches the unpinned runs. The policy, not a slower Typesense build, is the drop. v7 says this directly: waves of 100×1 plus pausing.

2. **The full 100 ms yield is paid when the topic is empty.** Pressure was fresh on 71–77% of samples during client runs, and 22% with no UI client. The yield A/B (pinned, one run, `BRAIAN_INDEXER_BULK_YIELD_MAX_MS=0`) shows the pause still matters when events are queued: edit→searchable p95 during the backlog was 204 ms with the pause and 310 ms without it. During the write it was 140 vs 158 ms. Zeroing the yield entirely is the wrong fix. Paying 100 ms on an empty topic is the waste.

3. **Schema width.** 518 filter/sort fields and 312 document keys on a 9.8 KB body. The microbench’s 56 KB payload is a different shape, so its docs/s are an upper-bound shape, not a prediction for this schema. Searchable-only copies were stored twice (column and `search_{lang}`). This branch stops storing the column. That removes 50 fields on the medium budget. It does not remove the 100 filterable and 40 sortable attributes the bench marks.

4. **Unpinned CPU sharing.** Pinned edit→searchable p95 is 140 ms; pool 8 is 375 ms; pool 12 is 258 ms. Same code, same data. The gap is the host, not the queue.

5. **Load-generator CPU when pinned.** v7 pinned Postgres-only median is 2240 obj/s with the loader on two cores. v6 called the same limit. It does not explain the indexer drop (the indexer is slower unpinned, where the loader is faster).

6. **API and Postgres writes are not the indexer limit.** 3825–4118 obj/s into Postgres against ~380 docs/s into Typesense. The relay was the earlier ceiling on small objects (about 178 events/s, later about 800) and is not what holds the 50k medium run.

## Proposals

Ordered by what to do next. Effect is against the v7 unpinned baseline of ~380 docs/s and edit→searchable p95 of 258–375 ms. Risk is to that latency or to durability.

### 1. Three-gear bulk waves — implemented

Idle: configured size, default 200×2, no wait. Pressure (HTTP in flight, interactive topic empty): configured batch at concurrency 1, wait `min(yield max, 15 ms)`. Events (topic pending or the interactive lane flushing): batch at most 100, concurrency 1, wait up to the yield max (default 100 ms). Caps stay 200 and 2. `BRAIAN_INDEXER_BULK_YIELD_MAX_MS=0` still imports immediately, including the pressure gear. A configured batch below the cap is a ceiling (`bulk_import_wave(150, 1, Idle)` stays 150×1).

Effect, estimated, not measured on the VPS. Pressure ~75% of waves: 200 documents. If 100 documents cost the 183 ms implied by v7 plus the microbench, 200 cost ~366 ms plus 15 ms ≈ 525 docs/s on that gear. Idle ~25%: the microbench’s 2×200 is ~768 docs/s on the heavier payload. A count-weighted blend of those two is about 600 docs/s. The 50 omitted fields are a small extra next to the 518 filter and sort columns that remain. If the live schema makes a 200-document import slower than twice a 100-document import, the blend lands in the mid-500s instead. Do not treat 600 as a new VPS result.

Latency: the events gear is the v7 protection (100×1 and the full pause) for an edit that is already queued. Pressure keeps concurrency 1 so one of the two Typesense threads stays free. The microbench’s upsert p95 beside 1×100 is 49 ms and beside 2×200 is 65 ms; beside 4×500 it is 197 ms. A 200-document in-flight import is longer than a 100-document one, so an upsert can pick up tens of milliseconds. That is not the v6 shape. The risk is a small regression in edit→searchable p95 during a long pressure wave, not a return to 1245 ms.

### 2. Do not store searchable-only attributes as columns — implemented

`stores_own_field` is filterable or sortable. Searchable-only text stays in `search_{lang}` and is not a schema column. The medium budget drops 50 fields (312 keys, 9 751 bytes, 518 own fields remain). UI list fields and the testdata `active` / `sku` columns stay. `a_name__sv` on the rules fixture is searchable-only and is absent; the text is in `search_sv`.

Effect: less payload and a narrower schema on every import and every upsert. Modest next to the wave change, and it removes the duplicate of long searchable text (`beskrivning`, max 4000). Risk: a client that read a searchable-only attribute off a search hit instead of Postgres will not find that key. The web app’s list and palette do not. Detail reads Postgres.

### 3. perf-load reads `BRAIAN_BULK_YIELD_*` — implemented

`BRAIAN_BULK_YIELD_MS`, `BRAIAN_BULK_YIELD_STEP_MS`, and `BRAIAN_BULK_YIELD_MAX_MS` apply, including `0`. Unset or unparsable keeps 1 / 4 / 25 ms. `BRAIAN_POOL_INTERACTIVE` is still ignored.

Effect: the yield A/B v7 could not run without a patch becomes a normal run. Risk: a shell that exports those variables for the API now also changes perf-load. That is the point of the variables.

### 4. Create-first import — not done

Skip the stored-seq search before a create-only batch. A 50k create would avoid the id lookups (chunks of 80) that run before every import, and those lookups compete with the visibility probe’s search. Updates and reindex would still need `replace_if_newer`, and a create that races an existing id falls into that slower path today (`already_exists` is retried as an update). Risk: a wrong fast path on reindex or on retry could overwrite a newer seq. Worth doing only with the seq check intact on the slow path, and measured separately.

### 5. Diagnostic run with no filter or sort fields — not done

The bench marks every third attribute filterable and a slice sortable. That is the benchmark definition; changing it would make the next report incomparable. `perf-load` already prints the three counts. A run with `--filterable 0 --sortable 0` would show how much of the ~180 ms per 100 documents is schema width. Risk: none to production. It is a measurement, not a default.

### 6. One more Typesense thread, still concurrency 1 under pressure — not done

Idle 200×2 fills both threads in the perf overlay. An edit that arrives during an idle wave has no free thread and waits out the import (the 2×200 row: upsert p95 65 ms, import 521 ms). Giving Typesense 3 threads and leaving the indexer at concurrency 2 when idle would keep one thread free. Risk: doing this by raising indexer concurrency above 2, or by dropping the concurrency-1 pressure gear, is the v6 regression. An ops experiment on the pinned host, not a code default.

### 7. Parallel indexer partitions — rejected

A second partition would allow a second bulk consumer and could push docs/s past one Typesense client. It also removes the “one partition so commit order survives a crash” property, and two consumers importing at once raise Typesense concurrency. That is how v6 reached 622 docs/s and 1245 ms p95. Not a safe change for this goal.

### 8. Replication factor greater than 1, plus a reconcile — not done

Produce with RF ≥ 3 (or RF 2 and `min.insync.replicas=2`) so an acked event survives a broker loss. A periodic job that compares `max(changes.seq)` with the highest seq Typesense has stored would catch the hole the current RF 1 leaves, without giving up the log. Effect: the log becomes a place slow jobs can read without a silent gap. Risk: a homelab compose with one Redpanda process cannot honour RF 3; the reconcile must be a read-only scan plus a reindex of the missing ids, not a second writer that races the indexer. Do this before relying on the log for jobs that must not miss an object.

### 9. Pinned CPU layout as the deploy picture — already measured

The v7 pinned column is the latency target (edit→searchable p95 140 ms) and the throughput compromise (bulk 2240 obj/s, indexer 477 docs/s). Documented in `docs/dev.md`. No code change. Risk: copying that overlay onto a machine with fewer than 8 cores prevents the containers from starting.

## What changed in this branch

The three implemented proposals above. No landing-page numbers. No new 50k run. The claim this review will stand behind after a VPS run is: indexer throughput moves back toward 600 docs/s, and edit→searchable p95 stays in the v7 band (about 140 ms pinned, 260–380 ms unpinned) rather than the v6 band (about 900–1250 ms). Until that run exists, the 600 docs/s figure is the blend in proposal 1.

## v8 follow-up

Measured on the same 8-vCPU VPS at `cbb2fa8` (the change above). Indexer throughput was 510 / 552 / 559 docs/s (pool 8 / pool 12 / pinned). The best 2-second windows reached about 940 docs/s, so the idle time between waves — the next Redpanda read and the document build — was most of the gap to the target. Edit→searchable p95 during COPY improved to 233 / 255 / 190 ms (v7: 375 / 258 / 140). During the backlog it got worse everywhere, 230–290 ms against v7’s 150–200 ms. That phase is the idle gear, which sent 200×2 and filled both Typesense threads. Switching the loader yield on or off moved edit→searchable by less than the ±80 ms noise.

The follow-up overlaps the next fetch and document build with the current import, and commits that next batch only after its own import. Idle waves start at 100×1 and grow only while the interactive topic stays empty. A second import call is allowed only after a full wave finished within 50 ms, which is the wait an upsert pays when both threads are busy. A queued edit resets the next wave to 100×1. This is not a new 50k measurement.
