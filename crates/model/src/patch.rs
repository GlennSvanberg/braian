use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::id::AttributeId;
use crate::language::LanguageTag;

/// A partial update of one object's values.
///
/// The patch is in object shape: language-dependent attributes carry only the
/// languages that change, and the other languages stay as they are.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ValuePatch {
    pub attributes: BTreeMap<AttributeId, AttributePatch>,
}

/// One attribute inside a [`ValuePatch`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttributePatch {
    /// Replace the stored value, including a whole language map.
    Replace { value: Value },
    /// Set these languages and leave any other languages of the attribute.
    Languages {
        values: BTreeMap<LanguageTag, Value>,
    },
}

/// Applies `patch` to `values`.
///
/// A language patch on a missing attribute starts a new language map. A language
/// patch on a value that is not a language map replaces that value.
pub fn apply_value_patch(values: &mut BTreeMap<AttributeId, Value>, patch: &ValuePatch) {
    for (id, change) in &patch.attributes {
        match change {
            AttributePatch::Replace { value } => {
                values.insert(*id, value.clone());
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
                    map.insert(language.as_str().to_owned(), value.clone());
                }
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
        languages.insert(LanguageTag::parse("sv").unwrap(), json!("ny"));
        apply_value_patch(
            &mut values,
            &ValuePatch {
                attributes: BTreeMap::from([(id, AttributePatch::Languages { values: languages })]),
            },
        );
        assert_eq!(values[&id], json!({"en-GB": "old", "sv": "ny"}));
    }
}
