use std::collections::BTreeMap;

use openpim_model::{
    Attribute, AttributeId, AttributeKind, DefinitionSet, GroupId, Object, ObjectId, StructureId,
    TypeId, UnitFamily,
};
use openpim_rules::CompiledModel;
use rand::RngExt;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use rust_decimal::Decimal;
use serde_json::{Map, Value};

use crate::Tier;
use crate::ids::Ids;

pub(crate) struct Prepared {
    pub(crate) model: CompiledModel,
    type_id: TypeId,
    groups: BTreeMap<StructureId, GroupId>,
    pub(crate) applicable: Vec<AttributeId>,
    pub(crate) key_attribute: AttributeId,
    /// Seed after the tier salt, so object bytes stay independent of tier mix-ups.
    pub(crate) seed: u64,
}

pub(crate) fn prepare(defs: &DefinitionSet, tier: Tier, seed: u64) -> Prepared {
    let model = CompiledModel::compile(defs).expect("synthetic definitions compile");
    let type_id = model.type_id("item").expect("type item");
    let catalog = model.structure_id("catalog").expect("structure catalog");
    let channel = model.structure_id("channel").expect("structure channel");
    let leaf = model.group_id("catalog", "leaf").expect("group leaf");
    let flat = model.group_id("channel", "flat").expect("group flat");
    let groups = BTreeMap::from([(catalog, leaf), (channel, flat)]);
    let applicable = model
        .applicable_ids(type_id, &groups)
        .expect("leaf placement");
    debug_assert_eq!(applicable.len(), tier.attribute_count());
    let key_attribute = model.key_attribute(type_id).expect("key attribute");
    let tier_salt = match tier {
        Tier::Light => 1,
        Tier::Medium => 2,
        Tier::Heavy => 3,
    };
    Prepared {
        model,
        type_id,
        groups,
        applicable,
        key_attribute,
        seed: seed.wrapping_add(tier_salt << 32),
    }
}

pub fn objects(defs: &DefinitionSet, tier: Tier, seed: u64, count: usize) -> Vec<Object> {
    objects_iter(defs, tier, seed, 0..count).collect()
}

/// Same objects as [`objects`], one at a time, for `range` of indexes.
///
/// Index 0 is the first object [`objects`] would return. Building index `n`
/// does not require building the ones before it.
pub fn objects_iter(
    defs: &DefinitionSet,
    tier: Tier,
    seed: u64,
    range: std::ops::Range<usize>,
) -> impl Iterator<Item = Object> + '_ {
    let prepared = prepare(defs, tier, seed);
    range.map(move |index| object_at(defs, &prepared, index))
}

pub(crate) fn object_at(defs: &DefinitionSet, prepared: &Prepared, index: usize) -> Object {
    let mut rng = ChaCha8Rng::seed_from_u64(
        prepared
            .seed
            .wrapping_add(index as u64)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15),
    );
    let mut chosen = prepared.applicable.clone();
    for cursor in (1..chosen.len()).rev() {
        let swap = rng.random_range(0..=cursor);
        chosen.swap(cursor, swap);
    }
    let target = (chosen.len() * 60 / 100).max(1);
    chosen.truncate(target);
    if !chosen.contains(&prepared.key_attribute) {
        chosen.pop();
        chosen.push(prepared.key_attribute);
    }
    chosen.sort_by_key(|id| {
        prepared
            .model
            .attribute(*id)
            .map(|attribute| attribute.key.clone())
            .unwrap_or_default()
    });

    let mut values = BTreeMap::new();
    for id in chosen {
        let attribute = prepared.model.attribute(id).expect("attribute");
        let value = if id == prepared.key_attribute {
            Value::String(format!("SKU-{index:08}"))
        } else {
            shaped(attribute, defs, &mut rng, index, 0)
        };
        values.insert(id, value);
    }

    Object {
        id: ObjectId::from_uuid(Ids::new(prepared.seed ^ (index as u64 + 1)).next()),
        type_id: prepared.type_id,
        model_revision: defs.model_revision,
        groups: prepared.groups.clone(),
        values,
    }
}

/// A value for `attribute` that uses `salt` so an edit does not repeat the
/// original sample. `salt` 0 is the value an object stores.
pub(crate) fn sampled_value(
    attribute: &Attribute,
    defs: &DefinitionSet,
    rng: &mut ChaCha8Rng,
    index: usize,
    salt: u64,
) -> Value {
    one(attribute, defs, rng, index, salt)
}

fn shaped(
    attribute: &Attribute,
    defs: &DefinitionSet,
    rng: &mut ChaCha8Rng,
    index: usize,
    salt: u64,
) -> Value {
    if attribute.language_dependent {
        let mut map = Map::new();
        for language in &defs.languages {
            map.insert(
                language.as_str().to_owned(),
                one(attribute, defs, rng, index, salt),
            );
        }
        Value::Object(map)
    } else {
        one(attribute, defs, rng, index, salt)
    }
}

fn one(
    attribute: &Attribute,
    defs: &DefinitionSet,
    rng: &mut ChaCha8Rng,
    index: usize,
    salt: u64,
) -> Value {
    if attribute.multiple {
        repeated(rng, |rng| scalar(attribute, defs, rng, index, salt))
    } else {
        scalar(attribute, defs, rng, index, salt)
    }
}

fn repeated(rng: &mut ChaCha8Rng, mut make: impl FnMut(&mut ChaCha8Rng) -> Value) -> Value {
    let count = rng.random_range(1..=3);
    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        items.push(make(rng));
    }
    Value::Array(items)
}

fn scalar(
    attribute: &Attribute,
    defs: &DefinitionSet,
    rng: &mut ChaCha8Rng,
    index: usize,
    salt: u64,
) -> Value {
    let shifted = index + salt as usize;
    match &attribute.kind {
        AttributeKind::Text { .. } => Value::String(text(rng)),
        AttributeKind::Number { unit_family, .. } => {
            let base = Decimal::from((shifted % 100) + 1);
            let Some(family_id) = unit_family else {
                return number_json(base, None);
            };
            let family = defs
                .unit_families
                .iter()
                .find(|family| family.id == *family_id)
                .expect("unit family");
            let unit = sample_unit(family, &attribute.key);
            let entered = if unit.code == family.base_unit {
                base
            } else {
                base.checked_div(unit.factor)
                    .expect("sample divides by the unit factor")
            };
            number_json(entered, Some(&unit.code))
        }
        AttributeKind::ValueList { options } => {
            let pick = rng.random_range(0..options.len());
            Value::String(options[pick].code.clone())
        }
        AttributeKind::Boolean => Value::Bool(rng.random()),
        AttributeKind::Date => {
            let month = (shifted % 12) + 1;
            let day = (shifted % 28) + 1;
            Value::String(format!("2024-{month:02}-{day:02}"))
        }
        AttributeKind::FileRef => Value::String(
            Ids::new(index as u64 ^ attribute.key.len() as u64 ^ salt)
                .next()
                .to_string(),
        ),
    }
}

fn sample_unit<'a>(family: &'a UnitFamily, key: &str) -> &'a openpim_model::Unit {
    let alternate = key.as_bytes().last().is_some_and(|byte| byte % 2 == 1);
    if alternate {
        family
            .units
            .iter()
            .find(|unit| unit.code != family.base_unit)
            .expect("alternate unit")
    } else {
        family
            .units
            .iter()
            .find(|unit| unit.code == family.base_unit)
            .expect("base unit")
    }
}

fn text(rng: &mut ChaCha8Rng) -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
    (0..32)
        .map(|_| {
            let index = rng.random_range(0..ALPHABET.len());
            ALPHABET[index] as char
        })
        .collect()
}

fn number_json(value: Decimal, unit: Option<&str>) -> Value {
    let mut map = Map::new();
    map.insert(
        "value".to_owned(),
        Value::String(value.normalize().to_string()),
    );
    if let Some(unit) = unit {
        map.insert("unit".to_owned(), Value::String(unit.to_owned()));
    }
    Value::Object(map)
}
