use std::collections::{BTreeMap, HashSet};
use std::ops::Range;
use std::str::FromStr;

use braian_model::{
    Attribute, AttributeId, AttributeKind, AttributePatch, DefinitionSet, LanguageTag, Object,
    ObjectId, ValuePatch,
};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use rust_decimal::Decimal;
use serde_json::Value;

use crate::Tier;
use crate::generate::{Prepared, object_at, prepare, sampled_value};

/// Which attributes an edit stream touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    /// One language-independent attribute, replaced wholesale.
    LanguageIndependent,
    /// One language-dependent attribute, in a single language.
    OneLanguage,
    /// Five attributes, mixing language-independent and language-dependent.
    Mixed,
}

impl EditKind {
    pub const ALL: [Self; 3] = [Self::LanguageIndependent, Self::OneLanguage, Self::Mixed];

    pub fn name(self) -> &'static str {
        match self {
            Self::LanguageIndependent => "language-independent",
            Self::OneLanguage => "one-language",
            Self::Mixed => "mixed",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "language-independent" => Some(Self::LanguageIndependent),
            "one-language" => Some(Self::OneLanguage),
            "mixed" => Some(Self::Mixed),
            _ => None,
        }
    }
}

/// A deterministic change to the object [`crate::objects`] would build at `index`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub index: usize,
    pub object_id: ObjectId,
    pub patch: ValuePatch,
}

/// Edits for `range` of object indexes.
///
/// The same seed and kind always yield the same patches. Every patch validates
/// against the synthetic definitions, and the new value differs from the value
/// the object was generated with when that attribute was filled.
pub fn edits(
    defs: &DefinitionSet,
    tier: Tier,
    seed: u64,
    kind: EditKind,
    range: Range<usize>,
) -> impl Iterator<Item = Edit> + '_ {
    let prepared = prepare(defs, tier, seed);
    range.map(move |index| edit_at(defs, &prepared, kind, index))
}

fn edit_at(defs: &DefinitionSet, prepared: &Prepared, kind: EditKind, index: usize) -> Edit {
    let object = object_at(defs, prepared, index);
    let language = &defs.languages[index % defs.languages.len()];
    let selected = select(prepared, &object, kind, index);
    let mut rng = ChaCha8Rng::seed_from_u64(
        prepared
            .seed
            .wrapping_add(index as u64)
            .wrapping_mul(0x6C62_272E_07BB_0142)
            ^ kind_salt(kind),
    );
    let mut attributes = BTreeMap::new();
    for id in selected {
        let attribute = prepared
            .model
            .attribute(id)
            .expect("selected attribute")
            .clone();
        let language = attribute.language_dependent.then_some(language);
        let mut value = sampled_value(&attribute, defs, &mut rng, index, 1);
        if current_leaf(&object, id, language).as_ref() == Some(&value) {
            value = nudge(&attribute, value);
        }
        let patch = match language {
            Some(language) => AttributePatch::Languages {
                values: BTreeMap::from([(language.clone(), Some(value))]),
            },
            None => AttributePatch::Replace { value },
        };
        attributes.insert(id, patch);
    }
    Edit {
        index,
        object_id: object.id,
        patch: ValuePatch { attributes },
    }
}

fn kind_salt(kind: EditKind) -> u64 {
    match kind {
        EditKind::LanguageIndependent => 0xA11C_E001,
        EditKind::OneLanguage => 0xB22C_E002,
        EditKind::Mixed => 0xC33C_E003,
    }
}

fn select(prepared: &Prepared, object: &Object, kind: EditKind, index: usize) -> Vec<AttributeId> {
    match kind {
        EditKind::LanguageIndependent => take(prepared, object, index, 1, false, &HashSet::new()),
        EditKind::OneLanguage => take(prepared, object, index, 1, true, &HashSet::new()),
        EditKind::Mixed => {
            let mut skip = HashSet::new();
            let mut chosen = take(prepared, object, index, 3, false, &skip);
            skip.extend(chosen.iter().copied());
            chosen.extend(take(prepared, object, index, 2, true, &skip));
            skip.extend(chosen.iter().copied());
            if chosen.len() < 5 {
                chosen.extend(take_any(prepared, object, index, 5 - chosen.len(), &skip));
            }
            chosen
        }
    }
}

/// Prefer attributes the object already has filled, then any applicable one.
fn take(
    prepared: &Prepared,
    object: &Object,
    index: usize,
    count: usize,
    language_dependent: bool,
    skip: &HashSet<AttributeId>,
) -> Vec<AttributeId> {
    let mut filled = Vec::new();
    let mut empty = Vec::new();
    for id in matching(prepared, language_dependent) {
        if skip.contains(&id) {
            continue;
        }
        if object.values.contains_key(&id) {
            filled.push(id);
        } else {
            empty.push(id);
        }
    }
    rotate(&mut filled, index);
    rotate(&mut empty, index);
    let chosen: Vec<_> = filled.into_iter().chain(empty).take(count).collect();
    assert!(
        chosen.len() == count,
        "synthetic tier has no attribute with language_dependent={language_dependent}"
    );
    chosen
}

fn take_any(
    prepared: &Prepared,
    object: &Object,
    index: usize,
    count: usize,
    skip: &HashSet<AttributeId>,
) -> Vec<AttributeId> {
    let mut ids = Vec::new();
    ids.extend(take_optional(prepared, object, index, count, false, skip));
    let mut skip = skip.clone();
    skip.extend(ids.iter().copied());
    if ids.len() < count {
        ids.extend(take_optional(
            prepared,
            object,
            index,
            count - ids.len(),
            true,
            &skip,
        ));
    }
    ids.truncate(count);
    ids
}

fn take_optional(
    prepared: &Prepared,
    object: &Object,
    index: usize,
    count: usize,
    language_dependent: bool,
    skip: &HashSet<AttributeId>,
) -> Vec<AttributeId> {
    let mut filled = Vec::new();
    let mut empty = Vec::new();
    for id in matching(prepared, language_dependent) {
        if skip.contains(&id) {
            continue;
        }
        if object.values.contains_key(&id) {
            filled.push(id);
        } else {
            empty.push(id);
        }
    }
    rotate(&mut filled, index);
    rotate(&mut empty, index);
    filled.into_iter().chain(empty).take(count).collect()
}

fn matching(prepared: &Prepared, language_dependent: bool) -> Vec<AttributeId> {
    let mut ids: Vec<_> = prepared
        .applicable
        .iter()
        .copied()
        .filter(|id| {
            *id != prepared.key_attribute
                && prepared
                    .model
                    .attribute(*id)
                    .is_some_and(|attribute| attribute.language_dependent == language_dependent)
        })
        .collect();
    ids.sort_by_key(|id| attribute_key(prepared, *id));
    ids
}

fn attribute_key(prepared: &Prepared, id: AttributeId) -> String {
    prepared
        .model
        .attribute(id)
        .map(|attribute| attribute.key.clone())
        .unwrap_or_default()
}

fn rotate(ids: &mut [AttributeId], index: usize) {
    if !ids.is_empty() {
        ids.rotate_left(index % ids.len());
    }
}

fn current_leaf(object: &Object, id: AttributeId, language: Option<&LanguageTag>) -> Option<Value> {
    let value = object.values.get(&id)?;
    match language {
        None => Some(value.clone()),
        Some(language) => value.get(language.as_str()).cloned(),
    }
}

fn nudge(attribute: &Attribute, value: Value) -> Value {
    if attribute.multiple {
        let mut items = match value {
            Value::Array(items) => items,
            other => vec![other],
        };
        if items.is_empty() {
            items.push(nudge_scalar(attribute, Value::Null));
        } else {
            let first = items[0].clone();
            items[0] = nudge_scalar(attribute, first);
        }
        Value::Array(items)
    } else {
        nudge_scalar(attribute, value)
    }
}

fn nudge_scalar(attribute: &Attribute, value: Value) -> Value {
    match &attribute.kind {
        AttributeKind::Text { max_length, .. } => {
            let mut text = value.as_str().unwrap_or("").to_owned();
            let max = max_length.unwrap_or(200) as usize;
            if text.len() < max {
                text.push('z');
            } else if let Some(last) = text.pop() {
                text.push(if last == 'z' { 'y' } else { 'z' });
            } else {
                text.push('z');
            }
            Value::String(text)
        }
        AttributeKind::Boolean => Value::Bool(!value.as_bool().unwrap_or(false)),
        AttributeKind::Number { .. } => {
            let mut map = value.as_object().cloned().unwrap_or_default();
            let current = map
                .get("value")
                .and_then(Value::as_str)
                .and_then(|text| Decimal::from_str(text).ok())
                .unwrap_or(Decimal::ZERO);
            map.insert(
                "value".to_owned(),
                Value::String((current + Decimal::ONE).normalize().to_string()),
            );
            Value::Object(map)
        }
        AttributeKind::Range { .. } => {
            let mut map = value.as_object().cloned().unwrap_or_default();
            let current = map
                .get("max")
                .and_then(Value::as_str)
                .and_then(|text| Decimal::from_str(text).ok())
                .unwrap_or(Decimal::ONE);
            map.insert(
                "max".to_owned(),
                Value::String((current + Decimal::ONE).normalize().to_string()),
            );
            Value::Object(map)
        }
        AttributeKind::ValueList { options, .. } => {
            let current = value.as_str().unwrap_or("");
            let next = options
                .iter()
                .find(|option| option.code != current)
                .map(|option| option.code.clone())
                .unwrap_or_else(|| {
                    options
                        .first()
                        .map(|option| option.code.clone())
                        .unwrap_or_default()
                });
            Value::String(next)
        }
        AttributeKind::Date => Value::String(bump_date(value.as_str().unwrap_or("2024-01-01"))),
        AttributeKind::ObjectRef { .. } | AttributeKind::FileRef { .. } => {
            let mut text = value.as_str().unwrap_or("").to_owned();
            match text.pop() {
                Some('a') => text.push('b'),
                Some(_) => text.push('a'),
                None => text.push('a'),
            }
            Value::String(text)
        }
    }
}

fn bump_date(text: &str) -> String {
    let mut parts = text.split('-');
    let year = parts.next().unwrap_or("2024");
    let month: u32 = parts.next().and_then(|part| part.parse().ok()).unwrap_or(1);
    let day: u32 = parts.next().and_then(|part| part.parse().ok()).unwrap_or(1);
    let (month, day) = if day < 28 {
        (month, day + 1)
    } else if month < 12 {
        (month + 1, 1)
    } else {
        (1, 1)
    };
    format!("{year}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use braian_model::apply_value_patch;
    use braian_rules::CompiledModel;

    use super::*;
    use crate::{definitions, objects, objects_iter};

    #[test]
    fn objects_iter_matches_objects() {
        let defs = definitions(Tier::Light, 1);
        let collected: Vec<_> = objects_iter(&defs, Tier::Light, 1, 0..4).collect();
        let direct = objects(&defs, Tier::Light, 1, 4);
        assert_eq!(
            serde_json::to_string(&collected).unwrap(),
            serde_json::to_string(&direct).unwrap()
        );
    }

    #[test]
    fn edits_are_deterministic_and_valid() {
        let defs = definitions(Tier::Light, 7);
        let model = CompiledModel::compile(&defs).unwrap();
        for kind in EditKind::ALL {
            let first: Vec<_> = edits(&defs, Tier::Light, 7, kind, 0..6).collect();
            let second: Vec<_> = edits(&defs, Tier::Light, 7, kind, 0..6).collect();
            assert_eq!(first, second, "{}", kind.name());
            for edit in &first {
                let mut object = object_at(&defs, &prepare(&defs, Tier::Light, 7), edit.index);
                assert_eq!(object.id, edit.object_id);
                let before = object.clone();
                apply_value_patch(&mut object.values, &edit.patch);
                assert_ne!(object.values, before.values);
                let errors = model.validate(&object);
                assert!(errors.is_empty(), "{kind:?} {errors:?}");
                match kind {
                    EditKind::LanguageIndependent => {
                        assert_eq!(edit.patch.attributes.len(), 1);
                        assert!(matches!(
                            edit.patch.attributes.values().next(),
                            Some(AttributePatch::Replace { .. })
                        ));
                    }
                    EditKind::OneLanguage => {
                        assert_eq!(edit.patch.attributes.len(), 1);
                        match edit.patch.attributes.values().next() {
                            Some(AttributePatch::Languages { values }) => {
                                assert_eq!(values.len(), 1);
                            }
                            other => panic!("expected one language, got {other:?}"),
                        }
                    }
                    EditKind::Mixed => assert_eq!(edit.patch.attributes.len(), 5),
                }
            }
        }
    }

    #[test]
    fn heavy_edit_validates() {
        let defs = definitions(Tier::Heavy, 1);
        let model = CompiledModel::compile(&defs).unwrap();
        let edit = edits(&defs, Tier::Heavy, 1, EditKind::Mixed, 0..1)
            .next()
            .unwrap();
        let mut object = object_at(&defs, &prepare(&defs, Tier::Heavy, 1), 0);
        apply_value_patch(&mut object.values, &edit.patch);
        let errors = model.validate(&object);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(edit.patch.attributes.len(), 5);
    }
}
