use std::collections::BTreeMap;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Context;
use braian_store::admin::{self, PgConn};
use braian_store::{Actor, BatchId, BatchKind, ObjectOp, PrincipalKind, StorageForm, Store};
use braian_synth::{EditKind, Tier, definitions, edits, objects_iter};
use serde::Serialize;
use serde_json::json;
use tokio_postgres::Client;

pub struct SpikeArgs {
    pub database_url: String,
    pub tier: Tier,
    pub objects: usize,
    pub edits: usize,
    pub chunk: usize,
    pub parallel: usize,
    pub seed: u64,
    pub out: PathBuf,
}

#[derive(Debug, Serialize)]
struct SpikeFile {
    tier: String,
    objects: usize,
    edits: usize,
    chunk: usize,
    parallel: usize,
    seed: u64,
    /// Write rate uses the slowest worker's write time, so generation is not included.
    rate_excludes_generation: bool,
    settings: BTreeMap<String, String>,
    forms: Vec<FormReport>,
}

#[derive(Debug, Serialize)]
struct FormReport {
    form: String,
    database: String,
    workloads: Vec<WorkloadReport>,
    tables_after_import: Vec<TableSize>,
}

#[derive(Debug, Serialize)]
struct WorkloadReport {
    name: String,
    objects: usize,
    transactions: usize,
    /// Slowest worker's summed write time.
    write_seconds: f64,
    objects_per_second: f64,
    wal_lsn_bytes: u64,
    wal_stat_bytes: u64,
    wal_fpi: u64,
    wal_lsn_bytes_per_object: u64,
    transaction_us_p50: u128,
    transaction_us_p95: u128,
    transaction_us_p99: u128,
}

#[derive(Debug, Serialize)]
struct TableSize {
    name: String,
    heap_bytes: i64,
    toast_bytes: i64,
    index_bytes: i64,
    total_bytes: i64,
}

struct WalStart {
    lsn: String,
}

pub async fn run(args: SpikeArgs) -> anyhow::Result<()> {
    if args.chunk == 0 {
        anyhow::bail!("chunk must be at least 1");
    }
    if args.edits.saturating_mul(6) > args.objects {
        anyhow::bail!(
            "{} edits times 6 workloads is more than {} loaded objects",
            args.edits,
            args.objects
        );
    }
    if let Some(parent) = args.out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let settings = read_settings(&args.database_url).await?;
    eprintln!("postgres settings: {settings:?}");
    let mut file = SpikeFile {
        tier: args.tier.name().to_owned(),
        objects: args.objects,
        edits: args.edits,
        chunk: args.chunk,
        parallel: args.parallel,
        seed: args.seed,
        rate_excludes_generation: true,
        settings,
        forms: Vec::new(),
    };
    for form in StorageForm::ALL {
        let report = measure_form(&args, form).await?;
        print_form(&report);
        file.forms.push(report);
        std::fs::write(&args.out, serde_json::to_string_pretty(&file)?)?;
    }
    println!("{}", serde_json::to_string_pretty(&summary_table(&file))?);
    eprintln!("wrote {}", args.out.display());
    Ok(())
}

async fn measure_form(args: &SpikeArgs, form: StorageForm) -> anyhow::Result<FormReport> {
    let database = format!("spike_{}", form.name().replace('-', "_"));
    eprintln!("\n=== {} in {database} ===", form.name());
    recreate(&args.database_url, &database).await?;
    let mut config = admin::parse_config(&args.database_url)?;
    config.dbname(&database);
    let store = Store::connect_config(config.clone(), form).await?;
    let meter = PgConn::connect(config).await?;
    meter
        .client
        .batch_execute(
            "ALTER TABLE objects SET (autovacuum_enabled = false);
             ALTER TABLE object_values SET (autovacuum_enabled = false);
             ALTER TABLE object_structures SET (autovacuum_enabled = false);
             DO $$
             DECLARE part record;
             BEGIN
                 FOR part IN
                     SELECT child.relname
                     FROM pg_inherits
                     JOIN pg_class parent ON parent.oid = inhparent
                     JOIN pg_class child ON child.oid = inhrelid
                     WHERE parent.relname = 'changes'
                 LOOP
                     EXECUTE format('ALTER TABLE %I SET (autovacuum_enabled = false)', part.relname);
                 END LOOP;
             END $$;",
        )
        .await?;

    let human = store
        .create_principal(PrincipalKind::Human, "spike")
        .await?;
    let actor = Actor {
        actor: human,
        on_behalf_of: None,
        kind: BatchKind::Bulk,
        source: "spike-1".into(),
    };
    let defs = Arc::new(definitions(args.tier, args.seed));
    store.put_definitions(&actor, &defs).await?;
    let batch = store.open_batch(&actor).await?;

    let import = run_workload(&meter.client, "import", args.objects, || {
        import_all(&store, &defs, args, batch, &actor)
    })
    .await?;
    let tables_after_import = table_sizes(&meter.client).await?;

    let interactive = Actor {
        kind: BatchKind::Interactive,
        ..actor.clone()
    };
    let mut workloads = vec![import];
    let mut cursor = 0usize;
    for kind in EditKind::ALL {
        let interactive_range = take_range(&mut cursor, args.edits);
        let chunk_range = take_range(&mut cursor, args.edits);
        workloads.push(
            run_workload(
                &meter.client,
                &format!("{} interactive", kind.name()),
                args.edits,
                || {
                    edit_interactive(
                        &store,
                        &defs,
                        args,
                        kind,
                        interactive_range.clone(),
                        &interactive,
                    )
                },
            )
            .await?,
        );
        workloads.push(
            run_workload(
                &meter.client,
                &format!("{} chunked", kind.name()),
                args.edits,
                || edit_chunked(&store, &defs, args, kind, chunk_range.clone(), &actor),
            )
            .await?,
        );
    }

    Ok(FormReport {
        form: form.name().to_owned(),
        database,
        workloads,
        tables_after_import,
    })
}

fn take_range(cursor: &mut usize, len: usize) -> Range<usize> {
    let start = *cursor;
    *cursor += len;
    start..*cursor
}

async fn run_workload<F, Fut>(
    client: &Client,
    name: &str,
    objects: usize,
    work: F,
) -> anyhow::Result<WorkloadReport>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<TimedWrites>>,
{
    eprintln!("  {name}: vacuum and checkpoint");
    let wal = wal_start(client).await?;
    let timed = work().await?;
    let (wal_lsn_bytes, wal_stat_bytes, wal_fpi) = wal_end(client, &wal).await?;
    let report = workload_report(name, objects, timed, wal_lsn_bytes, wal_stat_bytes, wal_fpi);
    eprintln!(
        "  {name}: {:.0} obj/s, WAL {:.1} KB/object, p95 {:.1} ms",
        report.objects_per_second,
        report.wal_lsn_bytes_per_object as f64 / 1024.0,
        report.transaction_us_p95 as f64 / 1000.0
    );
    Ok(report)
}

async fn import_all(
    store: &Store,
    defs: &Arc<braian_model::DefinitionSet>,
    args: &SpikeArgs,
    batch: BatchId,
    actor: &Actor,
) -> anyhow::Result<TimedWrites> {
    let mut joins = Vec::new();
    for range in split_range(0..args.objects, args.parallel) {
        let store = store.clone();
        let defs = Arc::clone(defs);
        let actor = actor.clone();
        let tier = args.tier;
        let seed = args.seed;
        let chunk = args.chunk;
        joins.push(tokio::spawn(async move {
            let mut times = Vec::new();
            let mut cursor = range.start;
            while cursor < range.end {
                let end = (cursor + chunk).min(range.end);
                let objects: Vec<_> = objects_iter(&defs, tier, seed, cursor..end).collect();
                cursor = end;
                let started = Instant::now();
                store.import_chunk(batch, &actor, &objects).await?;
                times.push(started.elapsed().as_micros());
            }
            anyhow::Ok(times)
        }));
    }
    join_times(joins).await
}

async fn edit_interactive(
    store: &Store,
    defs: &Arc<braian_model::DefinitionSet>,
    args: &SpikeArgs,
    kind: EditKind,
    range: Range<usize>,
    actor: &Actor,
) -> anyhow::Result<TimedWrites> {
    let mut joins = Vec::new();
    for worker in split_range(range, args.parallel) {
        let store = store.clone();
        let defs = Arc::clone(defs);
        let actor = actor.clone();
        let tier = args.tier;
        let seed = args.seed;
        joins.push(tokio::spawn(async move {
            let mut times = Vec::new();
            for edit in edits(&defs, tier, seed, kind, worker) {
                let started = Instant::now();
                store
                    .write_batch(
                        &actor,
                        vec![ObjectOp::Update {
                            id: edit.object_id,
                            patch: braian_model::ObjectPatch {
                                values: edit.patch,
                                groups: Default::default(),
                            },
                            base_version: 1,
                            read_version: 1,
                        }],
                    )
                    .await?;
                times.push(started.elapsed().as_micros());
            }
            anyhow::Ok(times)
        }));
    }
    join_times(joins).await
}

async fn edit_chunked(
    store: &Store,
    defs: &Arc<braian_model::DefinitionSet>,
    args: &SpikeArgs,
    kind: EditKind,
    range: Range<usize>,
    actor: &Actor,
) -> anyhow::Result<TimedWrites> {
    let batch = store.open_batch(actor).await?;
    let mut joins = Vec::new();
    for worker in split_range(range, args.parallel) {
        let store = store.clone();
        let defs = Arc::clone(defs);
        let actor = actor.clone();
        let tier = args.tier;
        let seed = args.seed;
        let chunk = args.chunk;
        joins.push(tokio::spawn(async move {
            let mut times = Vec::new();
            let mut cursor = worker.start;
            while cursor < worker.end {
                let end = (cursor + chunk).min(worker.end);
                let ops: Vec<_> = edits(&defs, tier, seed, kind, cursor..end)
                    .map(|edit| ObjectOp::Update {
                        id: edit.object_id,
                        patch: braian_model::ObjectPatch {
                            values: edit.patch,
                            groups: Default::default(),
                        },
                        base_version: 1,
                        read_version: 1,
                    })
                    .collect();
                cursor = end;
                let started = Instant::now();
                store.write_ops(batch, &actor, ops).await?;
                times.push(started.elapsed().as_micros());
            }
            anyhow::Ok(times)
        }));
    }
    join_times(joins).await
}

struct TimedWrites {
    /// Sum of write durations on the slowest worker. Generation is outside each sample.
    write_us: u128,
    times: Vec<u128>,
}

async fn join_times(
    joins: Vec<tokio::task::JoinHandle<anyhow::Result<Vec<u128>>>>,
) -> anyhow::Result<TimedWrites> {
    let mut workers = Vec::with_capacity(joins.len());
    for join in joins {
        workers.push(join.await??);
    }
    let write_us = workers
        .iter()
        .map(|times| times.iter().copied().sum::<u128>())
        .max()
        .unwrap_or(0);
    let times = workers.into_iter().flatten().collect();
    Ok(TimedWrites { write_us, times })
}

fn workload_report(
    name: &str,
    objects: usize,
    timed: TimedWrites,
    wal_lsn_bytes: u64,
    wal_stat_bytes: u64,
    wal_fpi: u64,
) -> WorkloadReport {
    let transactions = timed.times.len();
    let mut times = timed.times;
    times.sort_unstable();
    let write_seconds = timed.write_us as f64 / 1_000_000.0;
    let objects_per_second = if write_seconds > 0.0 {
        objects as f64 / write_seconds
    } else {
        0.0
    };
    WorkloadReport {
        name: name.to_owned(),
        objects,
        transactions,
        write_seconds,
        objects_per_second,
        wal_lsn_bytes,
        wal_stat_bytes,
        wal_fpi,
        wal_lsn_bytes_per_object: if objects == 0 {
            0
        } else {
            wal_lsn_bytes / objects as u64
        },
        transaction_us_p50: percentile(&times, 50.0),
        transaction_us_p95: percentile(&times, 95.0),
        transaction_us_p99: percentile(&times, 99.0),
    }
}

fn percentile(sorted: &[u128], p: f64) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn split_range(range: Range<usize>, parts: usize) -> Vec<Range<usize>> {
    let span = range.end.saturating_sub(range.start);
    if span == 0 {
        return Vec::new();
    }
    let parts = parts.max(1).min(span);
    let base = span / parts;
    let extra = span % parts;
    let mut cursor = range.start;
    let mut out = Vec::with_capacity(parts);
    for part in 0..parts {
        let len = base + usize::from(part < extra);
        out.push(cursor..cursor + len);
        cursor += len;
    }
    out
}

async fn recreate(database_url: &str, name: &str) -> anyhow::Result<()> {
    let mut config = admin::parse_config(database_url)?;
    config.dbname("postgres");
    let admin = PgConn::connect(config).await?;
    admin::drop_database(&admin.client, name).await.ok();
    admin::create_database(&admin.client, name).await?;
    Ok(())
}

async fn read_settings(database_url: &str) -> anyhow::Result<BTreeMap<String, String>> {
    let mut config = admin::parse_config(database_url)?;
    config.dbname("postgres");
    let admin = PgConn::connect(config).await?;
    let mut settings = BTreeMap::new();
    for name in [
        "wal_compression",
        "default_toast_compression",
        "synchronous_commit",
        "max_wal_size",
        "server_version",
    ] {
        let row = admin
            .client
            .query_one("SELECT current_setting($1)", &[&name])
            .await?;
        settings.insert(name.to_owned(), row.get(0));
    }
    Ok(settings)
}

async fn wal_start(client: &Client) -> anyhow::Result<WalStart> {
    client.batch_execute("SET lock_timeout = '60s'").await?;
    client.batch_execute("VACUUM").await?;
    client.batch_execute("CHECKPOINT").await?;
    client
        .batch_execute("SELECT pg_stat_reset_shared('wal')")
        .await?;
    let row = client
        .query_one("SELECT pg_current_wal_insert_lsn()::text", &[])
        .await?;
    Ok(WalStart { lsn: row.get(0) })
}

async fn wal_end(client: &Client, start: &WalStart) -> anyhow::Result<(u64, u64, u64)> {
    let row = client
        .query_one(
            "SELECT pg_wal_lsn_diff(pg_current_wal_insert_lsn(), start.lsn::pg_lsn)::bigint,
                    wal_bytes::bigint,
                    wal_fpi::bigint
             FROM pg_stat_wal
             CROSS JOIN (SELECT $1::text AS lsn) AS start",
            &[&start.lsn],
        )
        .await?;
    let lsn: i64 = row.get(0);
    let bytes: i64 = row.get(1);
    let fpi: i64 = row.get(2);
    Ok((u64_from(lsn)?, u64_from(bytes)?, u64_from(fpi)?))
}

async fn table_sizes(client: &Client) -> anyhow::Result<Vec<TableSize>> {
    let rows = client
        .query(
            "SELECT c.relname,
                    CASE WHEN c.relkind = 'p' THEN (
                        SELECT COALESCE(SUM(pg_relation_size(i.inhrelid)), 0)
                        FROM pg_inherits i WHERE i.inhparent = c.oid
                    ) ELSE pg_relation_size(c.oid) END::bigint AS heap_bytes,
                    CASE WHEN c.relkind = 'p' THEN (
                        SELECT COALESCE(SUM(pg_relation_size(child.reltoastrelid)), 0)
                        FROM pg_inherits i
                        JOIN pg_class child ON child.oid = i.inhrelid
                        WHERE i.inhparent = c.oid
                    ) ELSE COALESCE(pg_relation_size(NULLIF(c.reltoastrelid, 0)), 0) END::bigint AS toast_bytes,
                    CASE WHEN c.relkind = 'p' THEN (
                        SELECT COALESCE(SUM(pg_indexes_size(i.inhrelid)), 0)
                        FROM pg_inherits i WHERE i.inhparent = c.oid
                    ) ELSE pg_indexes_size(c.oid) END::bigint AS index_bytes,
                    CASE WHEN c.relkind = 'p' THEN (
                        SELECT COALESCE(SUM(pg_total_relation_size(i.inhrelid)), 0)
                        FROM pg_inherits i WHERE i.inhparent = c.oid
                    ) ELSE pg_total_relation_size(c.oid) END::bigint AS total_bytes
             FROM pg_class c
             JOIN pg_namespace n ON n.oid = c.relnamespace
             WHERE n.nspname = 'public'
               AND c.relname IN ('objects', 'object_values', 'object_structures', 'changes')
             ORDER BY c.relname",
            &[],
        )
        .await?;
    Ok(rows
        .iter()
        .map(|row| TableSize {
            name: row.get(0),
            heap_bytes: row.get(1),
            toast_bytes: row.get(2),
            index_bytes: row.get(3),
            total_bytes: row.get(4),
        })
        .collect())
}

fn u64_from(value: i64) -> anyhow::Result<u64> {
    u64::try_from(value).with_context(|| format!("negative counter {value}"))
}

fn print_form(report: &FormReport) {
    eprintln!("--- {} ---", report.form);
    for workload in &report.workloads {
        eprintln!(
            "{:<32} {:>8.0} obj/s  {:>8.1} KB WAL/obj  p95 {:>8.1} ms",
            workload.name,
            workload.objects_per_second,
            workload.wal_lsn_bytes_per_object as f64 / 1024.0,
            workload.transaction_us_p95 as f64 / 1000.0
        );
    }
}

fn summary_table(file: &SpikeFile) -> serde_json::Value {
    json!({
        "tier": file.tier,
        "objects": file.objects,
        "edits": file.edits,
        "parallel": file.parallel,
        "forms": file.forms.iter().map(|form| {
            json!({
                "form": form.form,
                "workloads": form.workloads,
            })
        }).collect::<Vec<_>>()
    })
}
