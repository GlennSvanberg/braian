mod demo;
mod spike;

use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use openpim_synth::Tier;

#[derive(Parser)]
#[command(
    name = "openpim-bench",
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
    }
    Ok(())
}

fn parse_tier(value: &str) -> Result<Tier, String> {
    Tier::from_name(value).ok_or_else(|| format!("expected light, medium or heavy, got {value}"))
}
