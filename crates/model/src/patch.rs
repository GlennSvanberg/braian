use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::id::{AttributeId, GroupId, StructureId};
use crate::language::LanguageTag;
use crate::object::Object;

/// A partial update of one object's values.
///
/// The patch is in object shape: language-dependent attributes carry only the
/// languages that change, and the other languages stay as they are.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ValuePatch {
    pub attributes: BTreeMap<AttributeId, AttributePatch>,
}

impl ValuePatch {
    pub fn is_empty(&self) -> bool {
        self.attributes.is_empty()
    }
}

/// One attribute inside a [`ValuePatch`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttributePatch {
    /// Replace the stored value, including a whole language map.
    Replace { value: Value },
    /// Set these languages and leave any other languages of the attribute.
    ///
    /// `None` removes that language.
    Languages {
        values: BTreeMap<LanguageTag, Option<Value>>,
    },
    /// Remove the attribute value, every language included.
    Remove,
}

/// A partial update of one object: values and, separately, where it sits.
///
/// `groups` merges per structure. `None` removes the object from that structure.
/// Structures that are absent from the map are left as they are.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ObjectPatch {
    #[serde(default, skip_serializing_if = "ValuePatch::is_empty")]
    pub values: ValuePatch,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub groups: BTreeMap<StructureId, Option<GroupId>>,
}

impl ObjectPatch {
    pub fn is_empty(&self) -> bool {
        self.values.is_empty() && self.groups.is_empty()
    }
}

/// Applies `patch` to `values`.
///
/// A language patch on a missing attribute starts a new language map. A language
/// patch on a value that is not a language map replaces that value. Removing the
/// last language removes the attribute.
pub fn apply_value_patch(values: &mut BTreeMap<AttributeId, Value>, patch: &ValuePatch) {
    for (id, change) in &patch.attributes {
        match change {
            AttributePatch::Replace { value } => {
                values.insert(*id, value.clone());
            }
            AttributePatch::Remove => {
                values.remove(id);
            }
            AttributePatch::Languages { values: languages } => {
                let entry = values
                    .entry(*id)
                    .or_insert_with(|| Value::Object(Map::new()));
                if !entry.is_object() {
                    *entry = Value::Object(Map::new());
                }
                let Some(map) = entry.as_object_mut() else {
                    continue;
                };
                for (language, value) in languages {
                    match value {
                        Some(value) => {
                            map.insert(language.as_str().to_owned(), value.clone());
                        }
                        None => {
                            map.remove(language.as_str());
                        }
                    }
                }
                if map.is_empty() {
                    values.remove(id);
                }
            }
        }
    }
}

/// Applies a value patch and a placement patch to one object.
pub fn apply_object_patch(object: &mut Object, patch: &ObjectPatch) {
    apply_value_patch(&mut object.values, &patch.values);
    for (structure, group) in &patch.groups {
        match group {
            Some(group) => {
                object.groups.insert(*structure, *group);
            }
            None => {
                object.groups.remove(structure);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn language_patch_keeps_the_other_languages() {
        let id = AttributeId::from_uuid(uuid::Uuid::nil());
        let mut values = BTreeMap::from([(id, json!({"sv": "gammal", "en-GB": "old"}))]);
        let mut languages = BTreeMap::new();
        languages.insert(LanguageTag::parse("sv").unwrap(), Some(json!("ny")));
        apply_value_patch(
            &mut values,
            &ValuePatch {
                attributes: BTreeMap::from([(id, AttributePatch::Languages { values: languages })]),
            },
        );
        assert_eq!(values[&id], json!({"en-GB": "old", "sv": "ny"}));
    }

    #[test]
    fn null_language_removes_only_that_language() {
        let id = AttributeId::from_uuid(uuid::Uuid::nil());
        let mut values = BTreeMap::from([(id, json!({"sv": "gammal", "en-GB": "old"}))]);
        let mut languages = BTreeMap::new();
        languages.insert(LanguageTag::parse("sv").unwrap(), None);
        apply_value_patch(
            &mut values,
            &ValuePatch {
                attributes: BTreeMap::from([(id, AttributePatch::Languages { values: languages })]),
            },
        );
        assert_eq!(values[&id], json!({"en-GB": "old"}));
    }

    #[test]
    fn remove_drops_the_attribute() {
        let id = AttributeId::from_uuid(uuid::Uuid::nil());
        let mut values = BTreeMap::from([(id, json!("SKU-1"))]);
        apply_value_patch(
            &mut values,
            &ValuePatch {
                attributes: BTreeMap::from([(id, AttributePatch::Remove)]),
            },
        );
        assert!(values.is_empty());
    }
}
