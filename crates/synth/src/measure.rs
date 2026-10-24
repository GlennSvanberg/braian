use std::collections::HashSet;
use std::time::Instant;

use braian_model::{AttributeId, DefinitionSet};
use braian_rules::CompiledModel;
use serde_json::{Map, Value};

use crate::Tier;
use crate::definitions::definitions;
use crate::generate::objects;

/// Compact-JSON size and validation time for one tier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TierMeasurement {
    pub tier: Tier,
    pub count: usize,
    pub filled: usize,
    pub languages: usize,
    pub definitions_json_bytes: usize,
    pub uuid_json: SampleStats,
    pub human_json: SampleStats,
    pub independent: SampleStats,
    pub language_part: SampleStats,
    pub language_parts_sum: SampleStats,
    pub validate_us: SampleStats,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SampleStats {
    pub mean: u64,
    pub p50: u64,
    pub p95: u64,
    pub max: u64,
}

pub fn measure(tier: Tier, seed: u64, count: usize) -> TierMeasurement {
    assert!(count > 0, "measure at least one object");
    let defs = definitions(tier, seed);
    let model = CompiledModel::compile(&defs).expect("synthetic definitions compile");
    let generated = objects(&defs, tier, seed, count);
    let localized: HashSet<AttributeId> = defs
        .attributes
        .iter()
        .filter(|attribute| attribute.language_dependent)
        .map(|attribute| attribute.id)
        .collect();
    let mut uuid_json = Vec::with_capacity(count);
    let mut human_json = Vec::with_capacity(count);
    let mut independent = Vec::with_capacity(count);
    let mut language_part = Vec::new();
    let mut language_parts_sum = Vec::with_capacity(count);
    let mut validate_us = Vec::with_capacity(count);

    for object in &generated {
        uuid_json.push(json_len(&serde_json::to_value(object).expect("object")));
        human_json.push(json_len(&human_values(object, &defs)));
        let (independent_part, per_language) = split(object, &defs, &localized);
        independent.push(json_len(&Value::Object(independent_part)));
        let mut sum = 0u64;
        for part in per_language {
            let bytes = json_len(&Value::Object(part));
            language_part.push(bytes);
            sum += bytes;
        }
        language_parts_sum.push(sum);
        let started = Instant::now();
        let errors = model.validate(object);
        let elapsed = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
        assert!(
            errors.is_empty(),
            "synthetic object failed validation: {errors:?}"
        );
        validate_us.push(elapsed);
    }

    TierMeasurement {
        tier,
        count,
        filled: generated[0].values.len(),
        languages: defs.languages.len(),
        definitions_json_bytes: serde_json::to_vec(&defs).expect("definitions").len(),
        uuid_json: stats(uuid_json),
        human_json: stats(human_json),
        independent: stats(independent),
        language_part: stats(language_part),
        language_parts_sum: stats(language_parts_sum),
        validate_us: stats(validate_us),
    }
}

impl TierMeasurement {
    pub fn report(&self) -> String {
        format!(
            "\
tier={name} count={count} filled={filled} languages={languages} definitions_json_bytes={definitions}
uuid_json_bytes        {uuid}
human_json_bytes       {human}
independent_part_bytes {independent}
language_part_bytes    {language}
all_language_parts     {sum}
validate_us            {validate}
",
            name = self.tier.name(),
            count = self.count,
            filled = self.filled,
            languages = self.languages,
            definitions = self.definitions_json_bytes,
            uuid = self.uuid_json,
            human = self.human_json,
            independent = self.independent,
            language = self.language_part,
            sum = self.language_parts_sum,
            validate = self.validate_us,
        )
    }
}

impl std::fmt::Display for SampleStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "mean={} p50={} p95={} max={}",
            self.mean, self.p50, self.p95, self.max
        )
    }
}

fn human_values(object: &braian_model::Object, defs: &DefinitionSet) -> Value {
    let mut values = Map::new();
    for (id, value) in &object.values {
        let key = defs
            .attributes
            .iter()
            .find(|attribute| attribute.id == *id)
            .map(|attribute| attribute.key.clone())
            .unwrap_or_else(|| id.to_string());
        values.insert(key, value.clone());
    }
    serde_json::json!({
        "id": object.id,
        "type_id": object.type_id,
        "model_revision": object.model_revision,
        "groups": object.groups,
        "values": values,
    })
}

fn split(
    object: &braian_model::Object,
    defs: &DefinitionSet,
    localized: &HashSet<AttributeId>,
) -> (Map<String, Value>, Vec<Map<String, Value>>) {
    let mut independent = Map::new();
    let mut per_language: Vec<Map<String, Value>> =
        defs.languages.iter().map(|_| Map::new()).collect();
    for (id, value) in &object.values {
        let key = id.to_string();
        if localized.contains(id) {
            let Some(map) = value.as_object() else {
                continue;
            };
            for (index, language) in defs.languages.iter().enumerate() {
                if let Some(inner) = map.get(language.as_str()) {
                    per_language[index].insert(key.clone(), inner.clone());
                }
            }
        } else {
            independent.insert(key, value.clone());
        }
    }
    (independent, per_language)
}

fn json_len(value: &Value) -> u64 {
    serde_json::to_vec(value).expect("json").len() as u64
}

fn stats(mut samples: Vec<u64>) -> SampleStats {
    assert!(!samples.is_empty());
    samples.sort_unstable();
    let count = samples.len();
    let sum: u128 = samples.iter().map(|sample| u128::from(*sample)).sum();
    let rank = |percent: usize| -> u64 {
        let index = (count * percent)
            .div_ceil(100)
            .saturating_sub(1)
            .min(count - 1);
        samples[index]
    };
    SampleStats {
        mean: (sum / count as u128) as u64,
        p50: rank(50),
        p95: rank(95),
        max: samples[count - 1],
    }
}
