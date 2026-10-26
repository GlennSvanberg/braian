# Change log

Postgres is the change log. There is no broker in the stack. A write commits a batch and appends rows to `changes`. The indexer and the live stream read those rows by `seq`.

`changes` stays append-only. Nothing updates an audit row to mark it consumed. That keeps the write path to one insert per change, and it keeps autovacuum from chasing a hot update on the log.

## Lanes

Each row has `kind`, either `interactive` or `bulk`. The two lanes have their own cursors in `index_lane`. A bulk backlog does not sit in front of an edit: the interactive worker claims the next interactive range while a bulk range is still open.

Both cursors are seeded from `indexer_progress.seq` when migration V15 runs. Events at or below that watermark are not replayed. If the search index might have missed anything below it, run `braian reindex`. `indexer_progress.seq` remains the “indexed through” watermark search uses (`GREATEST` after a successful import). The lane cursor is the durability position.

## Claims

A worker locks its lane row, drops expired unfinished claims, and leases the next free seq range (`index_claim`). The lease is 30 seconds and is renewed every 10 seconds while the import runs. The lock is only held while the range is assigned. Typesense work happens after the transaction commits, so two workers can import at the same time and still cannot hold the same sequences.

The cursor advances only across a contiguous prefix of finished claims. A later range that finishes first does not skip an earlier open range. A crash before commit leaves the lease to expire. The next worker claims the same sequences. Delivery is at-least-once. Typesense still refuses an older `seq` for an object it has already applied.

`seq` is allocated when the row is inserted, not when its transaction commits. A claim therefore reads every kind, not only its own, and stops before the first missing seq while another backend in this database holds a transaction id (`pg_stat_activity.backend_xid`) or a prepared transaction exists. That transaction can still commit the missing seq. `pg_snapshot_xip` is not enough to see it: on Postgres 16 the oldest active xid is folded into `xmin:xmax` and the in-progress list is empty. The same rule is what `changes_after` and the live tail use. The tail starts at `safe_change_seq`, the last seq of that prefix, rather than `max(seq)`.

A hole is closed with an already-done claim only when a second read still shows the hole and no other backend in this database holds a transaction id. Every missing seq is then aborted: identity values are not reused, and a transaction that starts afterwards can only take a seq above the ones already allocated. The closed hole is one insert and delete of a claim row, not an update of `changes`. Rows of the other lane that are already visible are skipped the same way, so a bulk backlog does not become interactive work.

An `xid8` column filled by `pg_current_xact_id()`, compared with `pg_snapshot_xmin`, does not draw this line. A transaction that starts first has the lower xid and can still insert a higher seq and commit while a later transaction holds a lower seq. That committed row has an xid below xmin, so a watermark on xmin steps over the open seq. The activity check is a read. COPY does not grow a column, a default, or an index.

Idle polls are a read (`COUNT` with a limit). They do not write the cursor. A claim takes at most 5 000 rows.

Several indexer processes can run. Each has its own holder id. They take disjoint ranges. `braian reindex` still rebuilds the Typesense alias from Postgres and does not depend on the log being complete.

## Wakeups

An `AFTER INSERT` row trigger on each `changes` partition runs `pg_notify('braian_changes', '')` once per transaction. A statement trigger on a partition does not fire for an insert routed through the parent, and the parent itself cannot carry a statement trigger. The trigger's `WHEN` clause checks a transaction-local flag, so a bulk `COPY` still notifies once. The payload is empty. Listeners re-read by seq. New monthly partitions get the same trigger from `ensure_change_partitions`.

The indexer and the API each hold a `LISTEN` connection. A missed notify is covered by a poll: 5 ms on the interactive lane when it is idle, 40 ms on bulk, and 250 ms on the live tail. `braian all` therefore keeps two listen connections. That is intentional.

The API takes `safe_change_seq` before it listens, then reads `changes` after that cursor. A commit that was still open, with a lower seq than the visible tip, stays in front of the cursor until it commits or aborts. The same process also emits locally after its own commit. Subscribers dedupe by `seq`. A commit from another connection, or another host, reaches the UI through the tail.

## Coalescing

Inside one claim, events for the same object collapse to the highest `seq`. The indexer then reads the current Postgres row and imports that. The interactive lane waits 20 ms to pick up a burst; the bulk lane waits 100 ms. Idle edit→searchable p50 on the perf stack was about 95 ms, which is that old shared 100 ms wait plus a short import. The interactive poll is still 5 ms. Bulk imports stay batched. The first bulk reads stay small and grow once the lane is warm, same as before.

Edit→searchable p95 during heavy COPY was about 345 ms, against about 222 ms when the log was Redpanda. The poll is not that gap. Part of it is the coalesce wait, which the shorter interactive window removes. The rest is Typesense serving the bulk import, and, after the visibility rule, an edit whose seq sits above a COPY transaction that has not committed yet. The cursor waits for that transaction. This tree did not re-run the 8-vCPU perf stack, so those two figures are the earlier run, not a new measurement.

Interactive commits still write through to Typesense via `index_after_commit`. That path does not move the lane cursor. If it drops a document, the claim path imports it.

## What a different transport would do

`braian-events` defines `ChangeSource`: pending, claim, commit, renew, wait. The only implementation is the Postgres reader in the indexer. A future transport would claim and commit the same way. None is shipped.

## Trade-offs

- The log is as durable as Postgres. There is no second copy of the event. A lost Postgres volume is a lost catalog, which was already true when the broker ran at replication factor 1.
- At-least-once means a crash can import the same object twice. The seq guard makes that safe for Typesense. A consumer that cannot tolerate a duplicate `seq` has to dedupe.
- The lane cursor can sit below the search watermark. Search “indexed through” is still `indexer_progress.seq`.
- `index_claim` stays about the size of in-flight work. Finished claims are deleted when the cursor folds over them. `changes` is retained as the audit trail. This design does not add a retention job.
- The extra cost on the write path is one more btree entry (`changes_kind_seq` on `(kind, seq)`) and, per row, the trigger `WHEN` check. The function body and the `NOTIFY` run once per transaction. There is no per-row update on the log.
- `LISTEN` is per database. Two Braian databases on one Postgres do not see each other’s notifies.

## Deploying over a host that still has the relay slot

Postgres will not start if `wal_level` is lowered while the logical slot `braian_relay` still exists. Drop the slot while the server is still running at `wal_level=logical`, then start the new compose file. `deploy/drop-relay-slot.sql` does the drop and refuses if the slot is still active. Stop `braian` first so the slot is idle. Migration V3 still creates the publication `braian_changes`. When `wal_level` is not logical that statement warns and still succeeds. Migration V15 drops the publication and also tries to drop the slot. If the slot is active, that step logs a notice and continues; the next Postgres restart will then fail until the slot is gone.
