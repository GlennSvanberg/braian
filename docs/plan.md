# Phase plan for the core

The design source is the Notion page
[Öppet PIM – tes och designprinciper](https://app.notion.com/p/403212bf181a81b4b96ed5d5dd0d77a7),
section "Kärnarkitektur". This file breaks Step 1 (the thin chain) into phases that are
built in order. Everything here is a direction and can change as measurements come in.

## How every phase is shaped

- **Demo:** `just demo-N` runs the phase's flow against the local stack (`just up`) and
  prints what happened. Anyone on the team can run it without knowing the code.
- **Test:** an end-to-end test runs the same flow in CI. If the demo breaks, CI fails.
- **Done when:** concrete exit criteria. Performance claims are backed by a file in
  `docs/measurements/`.
- **Spikes:** a spike test runs at the start of the phase whose design it decides, and its
  result is written to `docs/measurements/` before the code it decides is written.
- Postgres is the only source of truth. From phase 4 on, every demo can wipe Redpanda and
  Typesense and rebuild them from Postgres.

## Phase 0 – Skeleton (done)

Cargo workspace, compose stack (Postgres, Redpanda, Redpanda Console, Typesense),
devcontainer, CI and a read-only Postgres MCP.

- **Demo:** `just up` brings all services up healthy.
- **Test:** CI runs format, clippy, tests and the `wasm32-unknown-unknown` build of `rules`.

## Phase 1 – Definitions, rules and synthetic data (done)

No I/O. This phase fixes the vocabulary the rest of the system uses.

- **Decisions (Glenn 2026-10-24):**
  - Any attribute type can be multi-valued; the definition says so. Multiple values are an
    ordered list.
  - A structure group gets the classes of all its ancestor groups. This decides which
    attributes apply; values are never inherited.
  - Languages are BCP 47 codes with optional region (`sv`, `sv-SE`, `en-GB`). No fallback
    between languages in storage or index.
  - Numbers with units are stored as entered (value and unit). The index also gets the value
    normalized to the base unit, so filters and sorting work across units.
  - Definitions are authored through the API, and can be imported and exported as JSON or
    YAML files that live in git.
  - Synthetic tiers: light = 50 attributes and 3 languages, medium = 300 attributes and
    10 languages, heavy = 2000 attributes and 20 languages. In all tiers 30% of attributes
    are language-dependent and 60% of applicable attributes are filled.
- **Defaults (change if they turn out wrong):**
  - Numbers are exact decimals, not floats.
  - Value lists have a stable code and a label per language; the stored value is the code.
  - Text is plain text in v1. Rich text comes later.
  - Each type declares one key attribute, unique within the type and enforced by the database.
  - Validation errors are returned per attribute and language, with a machine-readable code.
- **Build:**
  - `model`: types, a global attribute pool, structures with groups and classes, and
    attributes marked as language-dependent or not.
  - Attribute types: text, number with unit, value list, boolean, date, file reference.
  - Index flags on attributes: searchable, filterable, sortable.
  - `rules`: validation of type, length, value list and unit. Rules live only on the
    attribute; a class only decides which attributes apply.
  - `rules-wasm`: the same rules exposed to JavaScript.
  - `synth`: a deterministic generator (same seed gives the same output) for a definition
    set plus light, medium and heavy objects. Heavy means up to 2000 attributes × 20 languages.
- **Demo:** `just demo-1` generates 10 heavy objects as JSON lines, validates them in Rust,
  then validates the same objects in Node through the WebAssembly build. Both report the
  same result, including for a few deliberately broken objects.
- **Test:**
  - Property tests: generated objects always validate; mutated objects fail with the
    expected error.
  - Shared cases in `testdata/rules/` run in both Rust and WebAssembly.
- **Done when:** object size in bytes per tier is recorded in `docs/measurements/`.

## Phase 2 – Storage in Postgres

- **Decisions (Glenn 2026-10-24):**
  - The 5,000 heavy objects per second target is measured as two workloads: full objects
    inserted (import) and edits of a few attributes on existing objects (enrichment).
  - Spike 1 compares three forms: one JSONB document per object, parts per language, and
    parts per language and attribute group.
  - Measurements run on the local Windows machine with Docker and count as indicative.
    Each measurement file records the hardware.
  - Deletes are in scope: a soft delete recorded as a change row, so the indexer can
    remove the object later.
  - Spike 1 (2026-10-24) stores values as one part per language and attribute group.
    Edits write about 11–32 KB of WAL instead of about 140 KB, and they were the
    fastest of the three forms. Import was fastest too. The cost is more WAL and
    more disk on a full import, because the parts stay in the heap. Numbers are in
    `docs/measurements/2026-10-24-spike-1-storage.md`.
- **Defaults (change if they turn out wrong):**
  - Definitions are stored in Postgres as versioned rows with the global model revision.
  - The key attribute is copied to a column on `objects` with a unique constraint per type.
  - `actor` and `on_behalf_of` reference a table of people, agents and integrations. An
    agent always has `on_behalf_of`.
  - The spike runs with `synchronous_commit=on`, `wal_compression=lz4` and lz4 TOAST
    compression, the same for every form.
  - `synth` also generates edit streams (one attribute, a few attributes, one language).
- **Spike 1:** measured. The store uses parts per language and attribute group.
- **Build:**
  - `store`: migrations for `objects`, `object_values` (one JSONB document per part),
    `object_structures` and `changes`. `changes` is both audit trail and outbox,
    partitioned by month.
  - Every write is a batch. Every change row records `actor` and `on_behalf_of`.
  - Read a single object with its current values.
  - Soft delete of an object as a change.
- **Demo:** `just demo-2` loads 100,000 synthetic objects, fetches one, and shows its rows in
  `changes`.
- **Test:** integration tests against a real Postgres, with a fresh database per test.
- **Done when:** spike 1 is measured and the chosen form is implemented.

## Phase 3 – API for writing and reading

- **Decisions:**
  - Concurrent edits merge per attribute and language. Two edits conflict only when they
    touch the same language of the same attribute, or the same language-independent
    attribute. A multi-valued attribute is one unit per language.
  - `PATCH` sends the changed values. JSON `null` clears a value or one language.
    `If-Match` is the object version the client started from. A missing `If-Match` on
    `PATCH` is 428. `DELETE` takes an optional `If-Match`; when it is sent, any change
    since that version is a conflict.
  - Every definition change is allowed, including a breaking one. A write is blocked
    only by errors on the attributes and languages it touches, plus type, key attribute
    and placement when the patch touches them. Errors that were already there come back
    as warnings, on writes and on `GET`.
  - Values for attributes that no longer apply stay in storage. `GET` hides them unless
    the client sends `?include=inapplicable`.
  - Definitions have per-entity endpoints and a whole-set import, JSON or YAML. One
    request is one batch and one model revision. A per-entity write that loses a
    revision race retries once. A whole-set import does not retry; it returns 409.
  - Object JSON accepts a key or an id. Responses use keys. Definition keys must not
    parse as UUIDs, so the two forms stay unambiguous. The client may send a UUIDv7 as
    the object id; otherwise the server generates one. One object per request.
  - Identity is a dev token from `OPENPIM_DEV_TOKENS`
    (`handle:kind[@behalf]:token`, separated by `;`). An agent token always names the
    person it acts for. Tokens are compared in constant time and are not logged.
  - OpenAPI is generated with utoipa and checked in. `just openapi` rewrites
    `docs/api/openapi.json`.
  - Attribute part groups are stable. Adding an attribute does not move an existing one
    to another part. A definition change stores the list of entities that changed, not
    the whole set.
- **Defaults:**
  - Routes live under `/v1`. Errors are `application/problem+json`. 422 is validation,
    409 is a conflict or a duplicate key, 410 is a deleted object.
  - A write response carries `version`, `seq`, `model_revision`, `warnings` and an `ETag`.
  - The API re-reads and validates at most three times when the object changed after it
    was read (`StaleRead`). The client's base version is only the conflict check.
  - `all` starts the API only, until relay and indexer exist.
- **Demo:** `just demo-3` creates `openpim_demo3`, serves the API on a free port, and runs
  `tests/hurl/phase-3` with Hurl.
- **Test:** API tests through the router against Postgres, the store conflict matrix, and
  a check that `docs/api/openapi.json` matches the code. The same Hurl files run in CI.
- **Done when:** `just demo-3` passes, the API tests and the OpenAPI check pass, and API
  latency is recorded in `docs/measurements/`.

## Phase 4 – Relay to Redpanda

- **Build:**
  - `relay` reads `changes` through logical replication, in commit order.
  - It publishes small events ("object X changed to version v") to two topics:
    interactive and bulk.
  - It exposes how far the replication slot lags, with a limit for alerting.
  - `events`: the event contract behind our own interface, using `rdkafka`.
- **Demo:** `just demo-4` saves through the API and shows the event in Redpanda Console.
  Then it stops the relay, writes 1,000 changes, restarts it, and shows that all 1,000
  arrive in commit order.
- **Test:**
  - No lost or reordered events across a relay crash.
  - The same suite also runs against Apache Kafka.

## Phase 5 – Indexer and search

- **Spike 2, first:** one Typesense collection per language versus one collection with a
  field per language. Measure RAM, import rate and full reindex time. Full reindex time is
  also the recovery time if Typesense is lost.
- **Build:**
  - `indexer` merges events per object over a short window, fetches current state from
    Postgres in bulk, and writes only attributes flagged for indexing. Interactive events
    are always taken before bulk.
  - It publishes the highest sequence number it has indexed.
  - `search`: our own search interface with a Typesense implementation.
  - Search and listing through our API, with an option to wait until the index has reached
    a given sequence number (read your own writes).
  - `openpim reindex` rebuilds from Postgres into a new collection and switches the alias.
- **Demo:** `just demo-5` saves an object, then searches for it using the sequence number
  the save returned. Then it deletes the Typesense data, runs reindex, and search works again.
- **Test:**
  - End-to-end from write to search.
  - Duplicate and out-of-date events produce the same index.

## Phase 6 – Scale proof

Spike 3 and Step 1.5 are the same thing: the whole chain under load.

- **Build:**
  - A bulk endpoint with progress and errors per row. A bulk job is a logical unit split
    into many smaller transactions.
  - `bench` drives the load and reports p50, p95 and p99.
- **Demo:** `just demo-6` runs a bulk job of heavy objects. While it runs, an interactive
  edit is made and timed until it is visible in search.
- **Done when:**
  - Sustained 5,000 heavy objects per second in bulk.
  - Interactive edits visible in search within 5 seconds (p95) while the bulk job runs.
  - Write rate, index lag and search latency for 1 million objects are recorded in
    `docs/measurements/`, then repeated for 10 million.

This closes Step 1.

## After the core (outline)

Each of these follows the same demo and test pattern when it is planned in detail.

- History and rollback: "object as of a date", "everything the agent changed yesterday",
  rollback by attribute, object or batch as a new append-only batch.
- Breaking definition changes: impact count ("4,312 objects affected"), conversion
  (map or drop), background migration and reindex through a new collection.
- Relations, variants and shared values, with async propagation to related search
  documents.
- Roles, and agent tokens tied to the person the agent acts for.
- Consumers of the same layer: UI (TanStack Start), realtime, webhooks, MCP and agent.
