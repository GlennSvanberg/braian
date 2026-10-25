#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

mod demo;
mod demo3;
mod latency;
mod perf_load;
mod spike;

use std::path::PathBuf;

use anyhow::Context;
use braian_synth::{IndexBudget, Tier};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "braian-bench",
    about = "Phase 2 storage measurements and demos"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Compare the three object storage forms. Writes docs-ready JSON.
    SpikeStorage {
        #[arg(long, env = "DATABASE_URL")]
        database_url: String,
        #[arg(long, value_parser = parse_tier, default_value = "heavy")]
        tier: Tier,
        /// Objects loaded per form. Edit workloads share this set.
        #[arg(long, default_value_t = 20_000)]
        objects: usize,
        /// Objects touched by each edit workload. Six workloads must fit in `objects`.
        #[arg(long, default_value_t = 2_000)]
        edits: usize,
        #[arg(long, default_value_t = 500)]
        chunk: usize,
        /// Parallel connections. Defaults to the number of CPUs.
        #[arg(long)]
        parallel: Option<usize>,
        #[arg(long, default_value_t = 1)]
        seed: u64,
        #[arg(long, default_value = "target/spike-1.json")]
        out: PathBuf,
    },
    /// Load synthetic objects, then read, edit and delete one.
    #[command(name = "demo-2")]
    Demo2 {
        #[arg(long, env = "DATABASE_URL")]
        database_url: String,
        #[arg(long, value_parser = parse_tier, default_value = "light")]
        tier: Tier,
        #[arg(long, default_value_t = 100_000)]
        count: usize,
        #[arg(long, default_value_t = 1)]
        seed: u64,
    },
    /// Import definitions, serve the API, and run tests/hurl/phase-3.
    #[command(name = "demo-3")]
    Demo3 {
        #[arg(long, env = "DATABASE_URL")]
        database_url: String,
    },
    /// Loopback HTTP latency against a direct store call, heavy tier.
    #[command(name = "api-latency")]
    ApiLatency {
        #[arg(long, env = "DATABASE_URL")]
        database_url: String,
        #[arg(long, default_value_t = 40)]
        samples: usize,
        #[arg(long, default_value_t = 1)]
        seed: u64,
    },
    /// Parallel bulk load, then write/read latency; with Typesense, change-to-searchable probes during the load.
    #[command(name = "perf-load")]
    PerfLoad {
        #[arg(long, env = "DATABASE_URL")]
        database_url: String,
        #[arg(long, value_parser = parse_tier, default_value = "medium")]
        tier: Tier,
        #[arg(long, default_value_t = 10_000)]
        count: usize,
        /// Parallel loaders, one connection each.
        #[arg(long, default_value_t = 8)]
        parallel: usize,
        #[arg(long, default_value_t = 250)]
        chunk: usize,
        #[arg(long, default_value_t = 1)]
        seed: u64,
        /// Keep only the first N searchable attributes.
        #[arg(long)]
        searchable: Option<usize>,
        #[arg(long)]
        filterable: Option<usize>,
        #[arg(long)]
        sortable: Option<usize>,
        /// Write the definitions and exit.
        #[arg(long)]
        define_only: bool,
        /// Generate every object before the clock starts, so the bulk number
        /// is store work only (no synthetic data generation on the hot path).
        #[arg(long)]
        pregenerate: bool,
        /// Typesense URL. Enables change-to-searchable probes.
        #[arg(long, env = "TYPESENSE_URL")]
        typesense_url: Option<String>,
        #[arg(long, env = "TYPESENSE_API_KEY", hide_env_values = true)]
        typesense_key: Option<String>,
        /// Minimum probes; probing also continues for the whole bulk.
        #[arg(long, default_value_t = 30)]
        probes: usize,
        #[arg(long, default_value_t = 200)]
        probe_interval_ms: u64,
        #[arg(long, default_value_t = 30)]
        probe_timeout_s: u64,
        /// Sequential PATCH/GET/read samples after the load. 0 skips them.
        #[arg(long, default_value_t = 200)]
        samples: usize,
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::SpikeStorage {
            database_url,
            tier,
            objects,
            edits,
            chunk,
            parallel,
            seed,
            out,
        } => {
            let parallel = parallel.unwrap_or_else(|| {
                std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(4)
            });
            spike::run(spike::SpikeArgs {
                database_url,
                tier,
                objects,
                edits,
                chunk,
                parallel,
                seed,
                out,
            })
            .await
            .context("spike-storage")?;
        }
        Command::Demo2 {
            database_url,
            tier,
            count,
            seed,
        } => demo::run(&database_url, tier, count, seed)
            .await
            .context("demo-2")?,
        Command::Demo3 { database_url } => demo3::run(&database_url).await.context("demo-3")?,
        Command::ApiLatency {
            database_url,
            samples,
            seed,
        } => latency::run(&database_url, samples, seed)
            .await
            .context("api-latency")?,
        Command::PerfLoad {
            database_url,
            tier,
            count,
            parallel,
            chunk,
            seed,
            searchable,
            filterable,
            sortable,
            define_only,
            pregenerate,
            typesense_url,
            typesense_key,
            probes,
            probe_interval_ms,
            probe_timeout_s,
            samples,
            out,
        } => perf_load::run(perf_load::PerfArgs {
            database_url,
            tier,
            count,
            parallel,
            chunk,
            seed,
            budget: IndexBudget {
                searchable,
                filterable,
                sortable,
            },
            define_only,
            pregenerate,
            typesense_url,
            typesense_key,
            probes,
            probe_interval: std::time::Duration::from_millis(probe_interval_ms),
            probe_timeout: std::time::Duration::from_secs(probe_timeout_s),
            samples,
            out,
        })
        .await
        .context("perf-load")?,
    }
    Ok(())
}

fn parse_tier(value: &str) -> Result<Tier, String> {
    Tier::from_name(value).ok_or_else(|| format!("expected light, medium or heavy, got {value}"))
}
