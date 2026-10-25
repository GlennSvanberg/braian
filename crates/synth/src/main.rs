use std::fs;
use std::path::PathBuf;

use anyhow::{Context, anyhow};
use braian_model::{DefinitionSet, Object};
use braian_rules::CompiledModel;
use braian_synth::{
    ExportFormat, IndexBudget, Tier, definitions, definitions_with, export, measure, objects,
    write_demo,
};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "braian-synth", about = "Deterministic synthetic Braian data")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Write a definition set as JSON or YAML.
    Definitions {
        #[arg(long, value_parser = parse_tier)]
        tier: Tier,
        /// Keep only the first N searchable attributes.
        #[arg(long)]
        searchable: Option<usize>,
        /// Keep only the first N filterable attributes.
        #[arg(long)]
        filterable: Option<usize>,
        /// Keep only the first N sortable attributes.
        #[arg(long)]
        sortable: Option<usize>,
        #[arg(long, default_value_t = 1)]
        seed: u64,
        #[arg(long, value_parser = parse_format, default_value = "json")]
        format: ExportFormat,
    },
    /// Write objects as JSON lines.
    Objects {
        #[arg(long, value_parser = parse_tier)]
        tier: Tier,
        #[arg(long)]
        count: usize,
        #[arg(long, default_value_t = 1)]
        seed: u64,
    },
    /// Validate JSON lines against a definition file.
    Validate {
        #[arg(long)]
        definitions: PathBuf,
        #[arg(long)]
        objects: PathBuf,
    },
    /// Print object size and validation time for every tier.
    Sizes {
        #[arg(long, default_value_t = 100)]
        count: usize,
        #[arg(long, default_value_t = 1)]
        seed: u64,
    },
    /// Write the phase 1 demo: 10 heavy objects and 4 broken ones.
    #[command(name = "demo-1")]
    Demo1 {
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 1)]
        seed: u64,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Definitions {
            tier,
            searchable,
            filterable,
            sortable,
            seed,
            format,
        } => {
            let budget = IndexBudget {
                searchable,
                filterable,
                sortable,
            };
            let text = export(&definitions_with(tier, seed, budget), format)?;
            println!("{text}");
        }
        Command::Objects { tier, count, seed } => {
            let defs = definitions(tier, seed);
            for object in objects(&defs, tier, seed, count) {
                println!("{}", serde_json::to_string(&object)?);
            }
        }
        Command::Validate {
            definitions,
            objects,
        } => validate_files(&definitions, &objects)?,
        Command::Sizes { count, seed } => {
            for tier in Tier::ALL {
                print!("{}", measure(tier, seed, count).report());
                println!();
            }
        }
        Command::Demo1 { out, seed } => {
            let summary = write_demo(&out, seed)?;
            let total: u64 = summary.rust_validate_us.iter().sum();
            println!(
                "demo-1: {} valid + {} broken heavy objects in {}",
                summary.valid,
                summary.broken,
                out.display()
            );
            println!("rust validate total {total} us");
            for (offset, code) in summary.broken_codes.iter().enumerate() {
                println!("broken object {}: {code}", summary.valid + offset);
            }
        }
    }
    Ok(())
}

fn validate_files(definitions: &std::path::Path, objects: &std::path::Path) -> anyhow::Result<()> {
    let text = fs::read_to_string(definitions)
        .with_context(|| format!("reading {}", definitions.display()))?;
    let defs: DefinitionSet = if definitions
        .extension()
        .is_some_and(|extension| extension == "yaml" || extension == "yml")
    {
        serde_norway::from_str(&text)?
    } else {
        serde_json::from_str(&text)?
    };
    let model = CompiledModel::compile(&defs).map_err(|errors| {
        anyhow!(
            "{}",
            errors
                .into_iter()
                .map(|error| error.message)
                .collect::<Vec<_>>()
                .join("; ")
        )
    })?;
    let body =
        fs::read_to_string(objects).with_context(|| format!("reading {}", objects.display()))?;
    let mut failed = 0usize;
    for (index, line) in body
        .lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
    {
        let object: Object = serde_json::from_str(line)
            .with_context(|| format!("object line {} is not JSON", index + 1))?;
        let errors = model.validate(&object);
        if !errors.is_empty() {
            failed += 1;
            println!("{index}\t{}", serde_json::to_string(&errors)?);
        }
    }
    if failed > 0 {
        anyhow::bail!("{failed} objects failed validation");
    }
    println!("validated objects: ok");
    Ok(())
}

fn parse_tier(value: &str) -> Result<Tier, String> {
    Tier::from_name(value).ok_or_else(|| format!("expected light, medium or heavy, got {value}"))
}

fn parse_format(value: &str) -> Result<ExportFormat, String> {
    ExportFormat::from_name(value).ok_or_else(|| format!("expected json or yaml, got {value}"))
}
