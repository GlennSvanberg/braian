//! Bulk load plus interactive probes against one Postgres database.
//!
//! Two measurements share this command:
//!
//! - Postgres write and read: parallel bulk loaders write `count` objects, then
//!   sequential HTTP `PATCH` + `GET` pairs, `GET` by id and 100-object reads run
//!   against an in-process API on the same store. Run it with relay and indexer off.
//! - Change to searchable: with `--typesense-url`, probes run *while the bulk is
//!   loading*. Each probe patches one searchable text attribute with a unique token
//!   through the API, then polls Typesense until that object is a hit.
//!
//! The database must be migrated (`Store::connect` does it) and empty of objects.
//! `--define-only` writes the definitions and exits, so the indexer can be started
//! on the final model before the load.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anyhow::Context;
use braian_model::{AttributeKind, DefinitionSet, Object, ObjectId};
use braian_search::{SearchQuery, Typesense};
use braian_store::admin;
use braian_store::{Actor, BatchKind, Error as StoreError, PoolConfig, STORAGE_FORM, Store};
use braian_synth::{IndexBudget, Tier, definitions_with, objects_iter};
use serde::Serialize;
use serde_json::{Map, Value, json};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use crate::latency::{HttpClient, percentile};

const TOKENS: &str = "perf:human:perf-bench-token";
const TOKEN: &str = "perf-bench-token";

pub struct PerfArgs {
    pub database_url: String,
    pub tier: Tier,
    pub count: usize,
    pub parallel: usize,
    pub chunk: usize,
    pub seed: u64,
    pub budget: IndexBudget,
    pub define_only: bool,
    pub pregenerate: bool,
    pub typesense_url: Option<String>,
    pub typesense_key: Option<String>,
    pub probes: usize,
    pub probe_interval: Duration,
    pub probe_timeout: Duration,
    pub samples: usize,
    pub out: Option<PathBuf>,
}

#[derive(Debug, Serialize, Default)]
struct Summary {
    tier: String,
    pregenerated: bool,
    count: usize,
    parallel: usize,
    chunk: usize,
    bulk_pool: usize,
    attributes: usize,
    languages: usize,
    searchable: usize,
    filterable: usize,
    sortable: usize,
    bulk_seconds: f64,
    bulk_objects_per_second: f64,
    patch_ms: Option<Stats>,
    patch_then_get_ms: Option<Stats>,
    get_by_id_ms: Option<Stats>,
    read_100_ms: Option<Stats>,
    read_100_objects_per_second: Option<f64>,
    probes_in_bulk: Option<Stats>,
    probes_all: Option<Stats>,
    probe_timeouts: usize,
    probe_patch_ms: Option<Stats>,
}

#[derive(Debug, Serialize)]
struct Stats {
    n: usize,
    p50: f64,
    p95: f64,
    p99: f64,
    max: f64,
}

fn stats(values: &[f64]) -> Option<Stats> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    Some(Stats {
        n: sorted.len(),
        p50: percentile(&sorted, 0.50),
        p95: percentile(&sorted, 0.95),
        p99: percentile(&sorted, 0.99),
        max: *sorted.last().expect("non-empty"),
    })
}

/// The attribute probes edit: the first searchable text attribute that is not the key.
struct ProbeTarget {
    attribute: String,
    language: Option<String>,
}

fn probe_target(defs: &DefinitionSet) -> anyhow::Result<ProbeTarget> {
    let key_attributes: Vec<_> = defs.types.iter().map(|t| t.key_attribute).collect();
    let attribute = defs
        .attributes
        .iter()
        .find(|attribute| {
            attribute.index.searchable
                && !attribute.multiple
                && !key_attributes.contains(&attribute.id)
                && matches!(attribute.kind, AttributeKind::Text { .. })
        })
        .context("no searchable text attribute to probe")?;
    Ok(ProbeTarget {
        attribute: attribute.id.as_uuid().to_string(),
        language: attribute
            .language_dependent
            .then(|| defs.languages[0].as_str().to_owned()),
    })
}

fn probe_body(target: &ProbeTarget, text: &str) -> Vec<u8> {
    let value = match &target.language {
        Some(language) => json!({ language: text }),
        None => Value::String(text.to_owned()),
    };
    let mut values = Map::new();
    values.insert(target.attribute.clone(), value);
    serde_json::to_vec(&json!({ "values": values })).expect("probe body")
}

pub async fn run(args: PerfArgs) -> anyhow::Result<()> {
    if args.chunk == 0 || args.parallel == 0 {
        anyhow::bail!("chunk and parallel must be positive");
    }
    let config = admin::parse_config(&args.database_url)?;
    let mut pools = PoolConfig::default();
    pools.bulk = bulk_pool_size(pools.bulk);
    let store = Store::connect_with(config, STORAGE_FORM, pools).await?;
    let bulk_pool = store.on_bulk().pool_max();
    let identities = braian_api::identities(&store, TOKENS).await?;
    let human = identities[0].actor.clone();

    let defs = definitions_with(args.tier, args.seed, args.budget);
    match store.definition_set().await {
        Err(StoreError::NoDefinitions) => {
            store.put_definitions(&human, &defs).await?;
        }
        Ok(existing) if existing == defs => {}
        Ok(_) => anyhow::bail!("the database already has other definitions; use an empty one"),
        Err(error) => return Err(error.into()),
    }
    let flags = |f: fn(&braian_model::IndexFlags) -> bool| {
        defs.attributes.iter().filter(|a| f(&a.index)).count()
    };
    let mut summary = Summary {
        tier: args.tier.name().to_owned(),
        count: args.count,
        parallel: args.parallel,
        chunk: args.chunk,
        bulk_pool,
        attributes: defs.attributes.len(),
        languages: defs.languages.len(),
        searchable: flags(|f| f.searchable),
        filterable: flags(|f| f.filterable),
        sortable: flags(|f| f.sortable),
        ..Summary::default()
    };
    println!(
        "perf-load: {} attributes, {} languages, {} searchable, {} filterable, {} sortable, bulk pool {}",
        summary.attributes,
        summary.languages,
        summary.searchable,
        summary.filterable,
        summary.sortable,
        summary.bulk_pool
    );
    if args.define_only {
        println!("definitions written");
        return Ok(());
    }

    let app = braian_api::router(store.clone(), identities);
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let defs = Arc::new(defs);
    let bulk_actor = Actor {
        actor: human.actor,
        on_behalf_of: None,
        kind: BatchKind::Bulk,
        source: "perf-load".into(),
    };
    let bulk_store = store.on_bulk();
    let batch = bulk_store.open_batch(&bulk_actor).await?;
    let loaded_ids: Arc<Mutex<Vec<ObjectId>>> = Arc::new(Mutex::new(Vec::new()));
    let loaded = Arc::new(AtomicUsize::new(0));
    let bulk_done = Arc::new(AtomicBool::new(false));
    let next_chunk = Arc::new(AtomicUsize::new(0));
    let chunks = args.count.div_ceil(args.chunk);

    let search = match (&args.typesense_url, &args.typesense_key) {
        (Some(url), Some(key)) => Some(Typesense::new(url.clone(), key.clone())),
        (Some(_), None) => anyhow::bail!("--typesense-key is required with --typesense-url"),
        _ => None,
    };
    let target = Arc::new(probe_target(&defs)?);
    let probes = search.clone().map(|search| {
        tokio::spawn(run_probes(
            search,
            Arc::clone(&target),
            port,
            Arc::clone(&loaded_ids),
            Arc::clone(&loaded),
            Arc::clone(&bulk_done),
            args.count,
            args.probes,
            args.probe_interval,
            args.probe_timeout,
            args.seed,
        ))
    });

    // Optional: build every chunk up front so the timed bulk is store work only.
    let ready: Arc<Vec<std::sync::Mutex<Option<Vec<Object>>>>> = if args.pregenerate {
        let generated = Instant::now();
        let mut jobs = Vec::with_capacity(chunks);
        for index in 0..chunks {
            let defs = Arc::clone(&defs);
            let (tier, seed, chunk, count) = (args.tier, args.seed, args.chunk, args.count);
            jobs.push(tokio::task::spawn_blocking(move || -> Vec<Object> {
                let start = index * chunk;
                let end = (start + chunk).min(count);
                objects_iter(&defs, tier, seed, start..end).collect()
            }));
        }
        let mut out = Vec::with_capacity(chunks);
        for job in jobs {
            out.push(std::sync::Mutex::new(Some(job.await?)));
        }
        println!(
            "pregenerated {} objects in {:.2}s",
            args.count,
            generated.elapsed().as_secs_f64()
        );
        Arc::new(out)
    } else {
        Arc::new(Vec::new())
    };
    summary.pregenerated = args.pregenerate;

    let started = Instant::now();
    let mut loaders = Vec::with_capacity(args.parallel);
    for _ in 0..args.parallel {
        let store = bulk_store.clone();
        let defs = Arc::clone(&defs);
        let actor = bulk_actor.clone();
        let next_chunk = Arc::clone(&next_chunk);
        let loaded = Arc::clone(&loaded);
        let loaded_ids = Arc::clone(&loaded_ids);
        let ready = Arc::clone(&ready);
        let (tier, seed, chunk, count) = (args.tier, args.seed, args.chunk, args.count);
        loaders.push(tokio::spawn(async move {
            // Build the next chunk on a blocking thread while this one is written.
            let generate = |index: usize| {
                let defs = Arc::clone(&defs);
                let ready = Arc::clone(&ready);
                tokio::task::spawn_blocking(move || -> Vec<Object> {
                    if let Some(slot) = ready.get(index) {
                        return slot.lock().expect("chunk slot").take().unwrap_or_default();
                    }
                    let start = index * chunk;
                    let end = (start + chunk).min(count);
                    objects_iter(&defs, tier, seed, start..end).collect()
                })
            };
            let mut pending = {
                let index = next_chunk.fetch_add(1, Ordering::Relaxed);
                (index < chunks).then(|| generate(index))
            };
            while let Some(job) = pending.take() {
                let objects = job.await?;
                let index = next_chunk.fetch_add(1, Ordering::Relaxed);
                pending = (index < chunks).then(|| generate(index));
                store.import_chunk(batch, &actor, &objects).await?;
                loaded.fetch_add(objects.len(), Ordering::Relaxed);
                loaded_ids
                    .lock()
                    .await
                    .extend(objects.iter().map(|object| object.id));
            }
            anyhow::Ok(())
        }));
    }
    let mut last_report = Instant::now();
    for loader in loaders {
        while !loader.is_finished() {
            tokio::time::sleep(Duration::from_millis(50)).await;
            if last_report.elapsed() >= Duration::from_secs(1) {
                let done = loaded.load(Ordering::Relaxed);
                println!(
                    "  {:>6.1}s loaded {done} ({:.0}/s)",
                    started.elapsed().as_secs_f64(),
                    done as f64 / started.elapsed().as_secs_f64()
                );
                last_report = Instant::now();
            }
        }
        loader.await??;
    }
    let bulk = started.elapsed();
    bulk_done.store(true, Ordering::Relaxed);
    summary.bulk_seconds = bulk.as_secs_f64();
    summary.bulk_objects_per_second = args.count as f64 / bulk.as_secs_f64();
    println!(
        "bulk: {} objects in {:.2}s = {:.0} obj/s ({} loaders, chunk {})",
        args.count,
        summary.bulk_seconds,
        summary.bulk_objects_per_second,
        args.parallel,
        args.chunk
    );

    if let Some(probes) = probes {
        let results = probes.await??;
        let in_bulk: Vec<f64> = results
            .iter()
            .filter(|r| r.in_bulk)
            .filter_map(|r| r.searchable_ms)
            .collect();
        let all: Vec<f64> = results.iter().filter_map(|r| r.searchable_ms).collect();
        let patch: Vec<f64> = results.iter().map(|r| r.patch_ms).collect();
        summary.probe_timeouts = results.iter().filter(|r| r.searchable_ms.is_none()).count();
        summary.probes_in_bulk = stats(&in_bulk);
        summary.probes_all = stats(&all);
        summary.probe_patch_ms = stats(&patch);
        print_stats("change->searchable in bulk", &summary.probes_in_bulk);
        print_stats("change->searchable all", &summary.probes_all);
        print_stats("probe patch", &summary.probe_patch_ms);
        println!("probe timeouts: {}", summary.probe_timeouts);
    }

    if args.samples > 0 {
        let ids = loaded_ids.lock().await.clone();
        read_write_samples(
            &store,
            &target,
            port,
            &ids,
            args.samples,
            args.seed,
            &mut summary,
        )
        .await?;
    }
    server.abort();

    let text = serde_json::to_string_pretty(&summary)?;
    if let Some(out) = &args.out {
        std::fs::write(out, format!("{text}\n"))
            .with_context(|| format!("writing {}", out.display()))?;
    }
    println!("{text}");
    Ok(())
}

/// `BRAIAN_POOL_BULK` when it is set, otherwise the code default.
///
/// `braian` reads the same variable. Other bench knobs stay on
/// [`PoolConfig::default`] so a shell export of yield timings does not
/// change a run.
fn bulk_pool_size(default: usize) -> usize {
    std::env::var("BRAIAN_POOL_BULK")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value: &usize| *value > 0)
        .unwrap_or(default)
}

fn print_stats(name: &str, stats: &Option<Stats>) {
    match stats {
        Some(s) => println!(
            "{name:<28} n {:>4}  p50 {:>8.1} ms  p95 {:>8.1} ms  p99 {:>8.1} ms  max {:>8.1} ms",
            s.n, s.p50, s.p95, s.p99, s.max
        ),
        None => println!("{name:<28} no samples"),
    }
}

/// Deterministic picks without a rand dependency in the bench crate.
fn pick(state: &mut u64, len: usize) -> usize {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    (*state % len as u64) as usize
}

async fn read_write_samples(
    store: &Store,
    target: &ProbeTarget,
    port: u16,
    ids: &[ObjectId],
    samples: usize,
    seed: u64,
    summary: &mut Summary,
) -> anyhow::Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let mut http = HttpClient::connect(port, TOKEN).await?;
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    // Objects the probes did not touch are still at version 1.
    let mut touched = std::collections::HashSet::new();
    let mut patch = Vec::with_capacity(samples);
    let mut patch_get = Vec::with_capacity(samples);
    for n in 0..samples {
        let id = loop {
            let id = ids[pick(&mut state, ids.len())];
            if touched.insert(id) {
                break id;
            }
        };
        let path = format!("/v1/objects/{}", id.as_uuid());
        let (_, before) = http.call("GET", &path, None).await?;
        let version = serde_json::from_slice::<Value>(&before)?
            .get("version")
            .and_then(Value::as_u64)
            .unwrap_or(1)
            .to_string();
        let body = probe_body(target, &format!("rw{seed}x{n}"));
        let started = Instant::now();
        let (status, response) = http
            .call_with("PATCH", &path, &[("if-match", &version)], Some(&body))
            .await?;
        let patched = started.elapsed();
        if status != 200 {
            anyhow::bail!(
                "patch returned {status}: {}",
                String::from_utf8_lossy(&response)
            );
        }
        let (status, _) = http.call("GET", &path, None).await?;
        let read = started.elapsed();
        if status != 200 {
            anyhow::bail!("get after patch returned {status}");
        }
        patch.push(patched.as_secs_f64() * 1e3);
        patch_get.push(read.as_secs_f64() * 1e3);
    }
    let mut get = Vec::with_capacity(samples);
    for _ in 0..samples {
        let id = ids[pick(&mut state, ids.len())];
        let path = format!("/v1/objects/{}", id.as_uuid());
        let started = Instant::now();
        let (status, _) = http.call("GET", &path, None).await?;
        get.push(started.elapsed().as_secs_f64() * 1e3);
        if status != 200 {
            anyhow::bail!("get returned {status}");
        }
    }
    let mut read_100 = Vec::with_capacity(samples);
    for _ in 0..samples {
        let picked: Vec<ObjectId> = (0..100).map(|_| ids[pick(&mut state, ids.len())]).collect();
        let started = Instant::now();
        let objects = store.get_objects(&picked).await?;
        read_100.push(started.elapsed().as_secs_f64() * 1e3);
        if objects.is_empty() {
            anyhow::bail!("bulk read returned nothing");
        }
    }
    summary.patch_ms = stats(&patch);
    summary.patch_then_get_ms = stats(&patch_get);
    summary.get_by_id_ms = stats(&get);
    summary.read_100_ms = stats(&read_100);
    summary.read_100_objects_per_second =
        summary.read_100_ms.as_ref().map(|s| 100.0 / (s.p50 / 1e3));
    print_stats("PATCH (http)", &summary.patch_ms);
    print_stats("PATCH then GET (http)", &summary.patch_then_get_ms);
    print_stats("GET by id (http)", &summary.get_by_id_ms);
    print_stats("read 100 by id (store)", &summary.read_100_ms);
    Ok(())
}

struct ProbeResult {
    in_bulk: bool,
    patch_ms: f64,
    searchable_ms: Option<f64>,
}

#[allow(clippy::too_many_arguments)]
async fn run_probes(
    search: Typesense,
    target: Arc<ProbeTarget>,
    port: u16,
    loaded_ids: Arc<Mutex<Vec<ObjectId>>>,
    loaded: Arc<AtomicUsize>,
    bulk_done: Arc<AtomicBool>,
    count: usize,
    want: usize,
    interval: Duration,
    timeout: Duration,
    seed: u64,
) -> anyhow::Result<Vec<ProbeResult>> {
    while loaded.load(Ordering::Relaxed) < (count / 20).max(1) {
        if bulk_done.load(Ordering::Relaxed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let mut state = seed.wrapping_mul(0xD1B5_4A32_D192_ED03) | 1;
    let mut used = std::collections::HashSet::new();
    let mut tasks = Vec::new();
    let mut n = 0usize;
    // Keep probing through the whole bulk, and at least `want` probes in total.
    while !bulk_done.load(Ordering::Relaxed) || n < want {
        let id = {
            let ids = loaded_ids.lock().await;
            if ids.is_empty() {
                None
            } else {
                let mut found = None;
                for _ in 0..32 {
                    let id = ids[pick(&mut state, ids.len())];
                    if used.insert(id) {
                        found = Some(id);
                        break;
                    }
                }
                found
            }
        };
        if let Some(id) = id {
            let in_bulk = !bulk_done.load(Ordering::Relaxed);
            let token = format!("probe{seed}q{n}z");
            tasks.push(tokio::spawn(probe(
                search.clone(),
                Arc::clone(&target),
                port,
                id,
                token,
                in_bulk,
                timeout,
            )));
            n += 1;
        }
        tokio::time::sleep(interval).await;
    }
    let mut results = Vec::with_capacity(tasks.len());
    for task in tasks {
        results.push(task.await??);
    }
    Ok(results)
}

async fn probe(
    search: Typesense,
    target: Arc<ProbeTarget>,
    port: u16,
    id: ObjectId,
    token: String,
    in_bulk: bool,
    timeout: Duration,
) -> anyhow::Result<ProbeResult> {
    let mut http = HttpClient::connect(port, TOKEN).await?;
    let path = format!("/v1/objects/{}", id.as_uuid());
    let body = probe_body(&target, &token);
    let started = Instant::now();
    let (status, response) = http
        .call_with("PATCH", &path, &[("if-match", "1")], Some(&body))
        .await?;
    let patch_ms = started.elapsed().as_secs_f64() * 1e3;
    if status != 200 {
        anyhow::bail!(
            "probe patch returned {status}: {}",
            String::from_utf8_lossy(&response)
        );
    }
    let wanted = id.as_uuid().to_string();
    let query = SearchQuery {
        text: token,
        language: target.language.clone().unwrap_or_else(|| "sv".into()),
        per_page: 10,
        page: 1,
        ..SearchQuery::default()
    };
    let searchable_ms = loop {
        if let Ok(page) = search.search(&query).await
            && page.hits.iter().any(|hit| hit.id == wanted)
        {
            break Some(started.elapsed().as_secs_f64() * 1e3);
        }
        if started.elapsed() > timeout {
            break None;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    Ok(ProbeResult {
        in_bulk,
        patch_ms,
        searchable_ms,
    })
}
