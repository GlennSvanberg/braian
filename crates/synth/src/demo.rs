use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::Instant;

use openpim_rules::CompiledModel;
use serde::Serialize;

use crate::Tier;
use crate::definitions::definitions;
use crate::generate::objects;
use crate::mutate::{Mutation, apply_mutation};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemoSummary {
    pub valid: usize,
    pub broken: usize,
    pub rust_validate_us: Vec<u64>,
    pub broken_codes: Vec<String>,
}

#[derive(Serialize)]
struct SummaryFile<'a> {
    tier: &'a str,
    seed: u64,
    valid: usize,
    broken: usize,
    rust_validate_us: &'a [u64],
    broken_codes: &'a [String],
}

pub fn write_demo(dir: &Path, seed: u64) -> std::io::Result<DemoSummary> {
    let defs = definitions(Tier::Heavy, seed);
    let model = CompiledModel::compile(&defs).expect("synthetic definitions compile");
    let mut generated = objects(&defs, Tier::Heavy, seed, 10);
    let base = generated[0].clone();
    let broken_codes = [
        Mutation::UnknownType,
        Mutation::TextTooLong,
        Mutation::MissingKey,
        Mutation::ValueNotInList,
    ]
    .into_iter()
    .map(|mutation| {
        let (object, expected) = apply_mutation(&base, &defs, mutation);
        let code = expected.code.to_string();
        generated.push(object);
        code
    })
    .collect::<Vec<_>>();

    std::fs::create_dir_all(dir)?;
    std::fs::write(
        dir.join("definitions.json"),
        serde_json::to_vec_pretty(&defs).expect("definitions"),
    )?;

    let mut objects_file = BufWriter::new(File::create(dir.join("objects.jsonl"))?);
    let mut reports_file = BufWriter::new(File::create(dir.join("rust-reports.jsonl"))?);
    let mut rust_validate_us = Vec::with_capacity(generated.len());
    for object in &generated {
        serde_json::to_writer(&mut objects_file, object).map_err(std::io::Error::other)?;
        objects_file.write_all(b"\n")?;
        let started = Instant::now();
        let errors = model.validate(object);
        rust_validate_us.push(u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX));
        serde_json::to_writer(&mut reports_file, &errors).map_err(std::io::Error::other)?;
        reports_file.write_all(b"\n")?;
    }
    objects_file.flush()?;
    reports_file.flush()?;

    let summary = DemoSummary {
        valid: 10,
        broken: broken_codes.len(),
        rust_validate_us,
        broken_codes,
    };
    let file = SummaryFile {
        tier: Tier::Heavy.name(),
        seed,
        valid: summary.valid,
        broken: summary.broken,
        rust_validate_us: &summary.rust_validate_us,
        broken_codes: &summary.broken_codes,
    };
    std::fs::write(
        dir.join("summary.json"),
        serde_json::to_vec_pretty(&file).expect("summary"),
    )?;
    Ok(summary)
}
