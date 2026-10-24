//! Per-attribute, per-language conflict checks.
//!
//! A change diff lists the attributes it touched. Two edits conflict when they
//! touch the same language of the same attribute, or the same language-independent
//! attribute. A unit with no language covers every language of that attribute.
//! Placement conflicts per structure.

use std::collections::{BTreeMap, BTreeSet};

use openpim_model::{AttributeId, AttributePatch, LanguageTag, ObjectPatch, StructureId};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::error::Conflict;
use crate::ids::PrincipalId;
use crate::split::{SplitContext, StorageForm, attr_key, language_of};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Unit {
    Value {
        attribute: AttributeId,
        language: Option<LanguageTag>,
    },
    Structure(StructureId),
}

#[derive(Debug, Clone)]
struct Touch {
    unit: Unit,
    version: u64,
    actor: PrincipalId,
    on_behalf_of: Option<PrincipalId>,
    current: Option<Value>,
}

pub(crate) fn conflicts_for(
    history: &[ChangeView],
    patch: &ObjectPatch,
    ctx: &SplitContext,
    form: StorageForm,
    before: &BTreeMap<String, Map<String, Value>>,
) -> Vec<Conflict> {
    let wanted = patch_units(ctx, form, patch, before);
    let mut matched: BTreeMap<Unit, Touch> = BTreeMap::new();
    for change in history {
        for touch in touches(change) {
            if wanted.iter().any(|unit| units_conflict(&touch.unit, unit)) {
                matched.insert(touch.unit.clone(), touch);
            }
        }
    }
    matched.into_values().map(conflict_from).collect()
}

/// Every unit touched since the base version. A delete conflicts with all of them.
pub(crate) fn all_conflicts(history: &[ChangeView]) -> Vec<Conflict> {
    let mut matched: BTreeMap<Unit, Touch> = BTreeMap::new();
    for change in history {
        for touch in touches(change) {
            matched.insert(touch.unit.clone(), touch);
        }
    }
    matched.into_values().map(conflict_from).collect()
}

#[derive(Debug, Clone)]
pub(crate) struct ChangeView {
    pub version: u64,
    pub actor: PrincipalId,
    pub on_behalf_of: Option<PrincipalId>,
    pub diff: Value,
}

fn patch_units(
    ctx: &SplitContext,
    form: StorageForm,
    patch: &ObjectPatch,
    before: &BTreeMap<String, Map<String, Value>>,
) -> BTreeSet<Unit> {
    let mut units = BTreeSet::new();
    for (id, change) in &patch.values.attributes {
        if ctx.is_localized(*id) {
            match change {
                AttributePatch::Languages { values } => {
                    for language in values.keys() {
                        units.insert(Unit::Value {
                            attribute: *id,
                            language: Some(language.clone()),
                        });
                    }
                }
                AttributePatch::Replace { value } => {
                    if let Some(map) = language_map(value) {
                        let mut languages: BTreeSet<String> = map.keys().cloned().collect();
                        languages.extend(stored_languages(ctx, form, before, *id));
                        for language in languages {
                            let Ok(language) = LanguageTag::parse(&language) else {
                                continue;
                            };
                            units.insert(Unit::Value {
                                attribute: *id,
                                language: Some(language),
                            });
                        }
                    } else {
                        units.insert(Unit::Value {
                            attribute: *id,
                            language: None,
                        });
                    }
                }
                AttributePatch::Remove => {
                    let languages = stored_languages(ctx, form, before, *id);
                    if languages.is_empty() {
                        units.insert(Unit::Value {
                            attribute: *id,
                            language: None,
                        });
                    }
                    for language in languages {
                        let Ok(language) = LanguageTag::parse(&language) else {
                            continue;
                        };
                        units.insert(Unit::Value {
                            attribute: *id,
                            language: Some(language),
                        });
                    }
                }
            }
        } else {
            units.insert(Unit::Value {
                attribute: *id,
                language: None,
            });
        }
    }
    for structure in patch.groups.keys() {
        units.insert(Unit::Structure(*structure));
    }
    units
}

fn stored_languages(
    ctx: &SplitContext,
    form: StorageForm,
    parts: &BTreeMap<String, Map<String, Value>>,
    id: AttributeId,
) -> BTreeSet<String> {
    let key = attr_key(id);
    let mut languages = BTreeSet::new();
    if form == StorageForm::Document || !ctx.is_localized(id) {
        let part = ctx.part_name(form, None, ctx.group_of(id));
        if let Some(map) = parts
            .get(&part)
            .and_then(|document| document.get(&key))
            .and_then(language_map)
        {
            languages.extend(map.keys().cloned());
        }
        return languages;
    }
    for (part, document) in parts {
        if document.contains_key(&key)
            && let Some(language) = language_of(form, part)
        {
            languages.insert(language.to_owned());
        }
    }
    languages
}

fn touches(change: &ChangeView) -> Vec<Touch> {
    let Some(diff) = change.diff.as_object() else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for (key, entry) in diff {
        let Some(entry) = entry.as_object() else {
            continue;
        };
        if key == "$groups" {
            for (structure, change_entry) in entry {
                let Ok(id) = Uuid::parse_str(structure) else {
                    continue;
                };
                let Some(body) = change_entry.as_object() else {
                    continue;
                };
                found.push(Touch {
                    unit: Unit::Structure(StructureId::from_uuid(id)),
                    version: change.version,
                    actor: change.actor,
                    on_behalf_of: change.on_behalf_of,
                    current: body.get("after").cloned(),
                });
            }
            continue;
        }
        let Ok(id) = Uuid::parse_str(key) else {
            continue;
        };
        let attribute = AttributeId::from_uuid(id);
        let before = entry.get("before").and_then(language_map);
        let after = entry.get("after").and_then(language_map);
        if before.is_some() || after.is_some() {
            let mut languages = BTreeSet::new();
            if let Some(map) = before {
                languages.extend(map.keys().cloned());
            }
            if let Some(map) = after {
                languages.extend(map.keys().cloned());
            }
            for language in languages {
                let Ok(tag) = LanguageTag::parse(&language) else {
                    continue;
                };
                let current = after.and_then(|map| map.get(&language)).cloned();
                found.push(Touch {
                    unit: Unit::Value {
                        attribute,
                        language: Some(tag),
                    },
                    version: change.version,
                    actor: change.actor,
                    on_behalf_of: change.on_behalf_of,
                    current,
                });
            }
        } else {
            found.push(Touch {
                unit: Unit::Value {
                    attribute,
                    language: None,
                },
                version: change.version,
                actor: change.actor,
                on_behalf_of: change.on_behalf_of,
                current: entry.get("after").cloned(),
            });
        }
    }
    found
}

fn units_conflict(left: &Unit, right: &Unit) -> bool {
    match (left, right) {
        (Unit::Structure(left), Unit::Structure(right)) => left == right,
        (
            Unit::Value {
                attribute: left,
                language: left_language,
            },
            Unit::Value {
                attribute: right,
                language: right_language,
            },
        ) => {
            left == right
                && (left_language.is_none()
                    || right_language.is_none()
                    || left_language == right_language)
        }
        _ => false,
    }
}

fn language_map(value: &Value) -> Option<&Map<String, Value>> {
    let map = value.as_object()?;
    if map.is_empty() || map.keys().any(|key| LanguageTag::parse(key).is_err()) {
        None
    } else {
        Some(map)
    }
}

fn conflict_from(touch: Touch) -> Conflict {
    let (attribute, language, structure) = match &touch.unit {
        Unit::Value {
            attribute,
            language,
        } => (Some(*attribute), language.clone(), None),
        Unit::Structure(structure) => (None, None, Some(*structure)),
    };
    Conflict {
        attribute,
        language,
        structure,
        version: touch.version,
        actor: touch.actor,
        on_behalf_of: touch.on_behalf_of,
        current: touch.current,
    }
}

#[cfg(test)]
mod tests {
    use openpim_model::{AttributePatch, LanguageTag, ObjectPatch, ValuePatch};
    use serde_json::json;
    use uuid::Uuid;

    use super::*;

    fn tag(value: &str) -> LanguageTag {
        LanguageTag::parse(value).unwrap()
    }

    fn value_unit(attribute: AttributeId, language: Option<&str>) -> Unit {
        Unit::Value {
            attribute,
            language: language.map(tag),
        }
    }

    #[test]
    fn different_languages_of_one_attribute_do_not_conflict() {
        assert!(!units_conflict(
            &value_unit(AttributeId::from_uuid(Uuid::nil()), Some("sv")),
            &value_unit(AttributeId::from_uuid(Uuid::nil()), Some("en-GB"))
        ));
    }

    #[test]
    fn the_same_language_conflicts_and_a_whole_value_covers_every_language() {
        let id = AttributeId::from_uuid(Uuid::from_u128(1));
        assert!(units_conflict(
            &value_unit(id, Some("sv")),
            &value_unit(id, Some("sv"))
        ));
        assert!(units_conflict(
            &value_unit(id, None),
            &value_unit(id, Some("sv"))
        ));
        assert!(!units_conflict(
            &value_unit(id, None),
            &value_unit(AttributeId::from_uuid(Uuid::from_u128(2)), None)
        ));
    }

    #[test]
    fn a_language_diff_becomes_one_unit_per_language() {
        let id = AttributeId::from_uuid(Uuid::from_u128(3));
        let change = ChangeView {
            version: 4,
            actor: PrincipalId::from_uuid(Uuid::nil()),
            on_behalf_of: None,
            diff: json!({
                id.as_uuid().to_string(): {
                    "before": {"sv": "gammal"},
                    "after": {"sv": "ny", "en-GB": null}
                }
            }),
        };
        let units: BTreeSet<_> = touches(&change)
            .into_iter()
            .map(|touch| touch.unit)
            .collect();
        assert_eq!(
            units,
            BTreeSet::from([value_unit(id, Some("sv")), value_unit(id, Some("en-GB"))])
        );
    }

    #[test]
    fn patch_units_follow_the_languages_in_the_patch() {
        let defs = openpim_synth::definitions(openpim_synth::Tier::Light, 1);
        let ctx = SplitContext::new(&defs);
        let id = defs
            .attributes
            .iter()
            .find(|attribute| attribute.language_dependent)
            .unwrap()
            .id;
        let mut values = BTreeMap::new();
        values.insert(tag("en-GB"), Some(json!("Chair")));
        let patch = ObjectPatch {
            values: ValuePatch {
                attributes: BTreeMap::from([(id, AttributePatch::Languages { values })]),
            },
            groups: BTreeMap::new(),
        };
        let units = patch_units(
            &ctx,
            StorageForm::PerLanguageGroup,
            &patch,
            &BTreeMap::new(),
        );
        assert!(units.contains(&value_unit(id, Some("en-GB"))));
        assert!(!units.contains(&value_unit(id, Some("sv"))));
    }
}
