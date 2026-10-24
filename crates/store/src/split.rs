use std::collections::{BTreeMap, BTreeSet, HashMap};

use openpim_model::{AttributeId, AttributePatch, DefinitionSet, ValuePatch};
use serde_json::{Map, Value};
use uuid::Uuid;

/// How an object's values are divided across `object_values` rows.
///
/// The table is the same for every form. Spike 1 measures these three and
/// [`crate::STORAGE_FORM`] is the one [`crate::Store`] writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageForm {
    /// One JSONB document. Language maps stay inside the attribute value.
    Document,
    /// Language-independent values in part `""`, and one part per language tag.
    PerLanguage,
    /// Same as per language, then split again into blocks of attributes.
    PerLanguageGroup,
}

impl StorageForm {
    pub const ALL: [Self; 3] = [Self::Document, Self::PerLanguage, Self::PerLanguageGroup];

    pub fn name(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::PerLanguage => "per-language",
            Self::PerLanguageGroup => "per-language-group",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|form| form.name() == name)
    }
}

/// Attribute facts the split needs. Built once from a definition set.
#[derive(Debug, Clone)]
pub struct SplitContext {
    language_dependent: HashMap<AttributeId, bool>,
    /// Block of ~100 attributes, ordered by attribute key.
    group: HashMap<AttributeId, usize>,
    languages: Vec<String>,
}

const GROUP_SIZE: usize = 100;

impl SplitContext {
    pub fn new(defs: &DefinitionSet) -> Self {
        let mut order: Vec<_> = defs.attributes.iter().collect();
        order.sort_by(|left, right| {
            left.key
                .cmp(&right.key)
                .then(left.id.as_uuid().cmp(&right.id.as_uuid()))
        });
        let group = order
            .into_iter()
            .enumerate()
            .map(|(index, attribute)| (attribute.id, index / GROUP_SIZE))
            .collect();
        let language_dependent = defs
            .attributes
            .iter()
            .map(|attribute| (attribute.id, attribute.language_dependent))
            .collect();
        Self {
            language_dependent,
            group,
            languages: defs
                .languages
                .iter()
                .map(|language| language.as_str().to_owned())
                .collect(),
        }
    }

    #[cfg(test)]
    pub fn group_count(&self) -> usize {
        self.group
            .values()
            .max()
            .map(|group| group + 1)
            .unwrap_or(0)
    }

    fn is_localized(&self, id: AttributeId) -> bool {
        self.language_dependent.get(&id).copied().unwrap_or(false)
    }

    fn group_of(&self, id: AttributeId) -> usize {
        self.group.get(&id).copied().unwrap_or(0)
    }

    pub fn part_name(&self, form: StorageForm, language: Option<&str>, group: usize) -> String {
        match form {
            StorageForm::Document => String::new(),
            StorageForm::PerLanguage => language.unwrap_or("").to_owned(),
            StorageForm::PerLanguageGroup => match language {
                Some(language) => format!("{language}#{group}"),
                None => format!("#{group}"),
            },
        }
    }

    /// Parts a patch has to lock. A language replace also names every known
    /// language so a removed language is cleared from its part.
    pub fn affected_parts(&self, form: StorageForm, patch: &ValuePatch) -> Vec<String> {
        if form == StorageForm::Document {
            return vec![String::new()];
        }
        let mut parts = BTreeSet::new();
        for (id, change) in &patch.attributes {
            let group = self.group_of(*id);
            match change {
                AttributePatch::Replace { value } if self.is_localized(*id) => {
                    if let Some(map) = value.as_object() {
                        for language in map.keys() {
                            parts.insert(self.part_name(form, Some(language), group));
                        }
                    }
                    for language in &self.languages {
                        parts.insert(self.part_name(form, Some(language), group));
                    }
                }
                AttributePatch::Replace { .. } => {
                    parts.insert(self.part_name(form, None, group));
                }
                AttributePatch::Languages { values } => {
                    for language in values.keys() {
                        parts.insert(self.part_name(form, Some(language.as_str()), group));
                    }
                }
            }
        }
        parts.into_iter().collect()
    }

    pub fn split(
        &self,
        form: StorageForm,
        values: &BTreeMap<AttributeId, Value>,
    ) -> BTreeMap<String, Map<String, Value>> {
        let mut parts = BTreeMap::new();
        for (id, value) in values {
            self.insert_value(form, &mut parts, *id, value);
        }
        parts
    }

    #[cfg(test)]
    pub fn join(
        &self,
        form: StorageForm,
        parts: &BTreeMap<String, Value>,
    ) -> Result<BTreeMap<AttributeId, Value>, String> {
        join_parts(form, parts)
    }

    /// Applies `patch` to the parts that were loaded for it.
    pub fn apply_patch(
        &self,
        form: StorageForm,
        parts: &mut BTreeMap<String, Map<String, Value>>,
        patch: &ValuePatch,
    ) {
        for (id, change) in &patch.attributes {
            let key = attr_key(*id);
            match change {
                AttributePatch::Replace { value }
                    if self.is_localized(*id) && form != StorageForm::Document =>
                {
                    for document in parts.values_mut() {
                        document.remove(&key);
                    }
                    self.insert_value(form, parts, *id, value);
                }
                AttributePatch::Replace { value } => {
                    let part = self.part_name(form, None, self.group_of(*id));
                    parts.entry(part).or_default().insert(key, value.clone());
                }
                AttributePatch::Languages { values } if form == StorageForm::Document => {
                    let document = parts.entry(String::new()).or_default();
                    let slot = document
                        .entry(key)
                        .or_insert_with(|| Value::Object(Map::new()));
                    if !slot.is_object() {
                        *slot = Value::Object(Map::new());
                    }
                    if let Some(map) = slot.as_object_mut() {
                        for (language, inner) in values {
                            map.insert(language.as_str().to_owned(), inner.clone());
                        }
                    }
                }
                AttributePatch::Languages { values } => {
                    let group = self.group_of(*id);
                    for (language, inner) in values {
                        let part = self.part_name(form, Some(language.as_str()), group);
                        parts
                            .entry(part)
                            .or_default()
                            .insert(key.clone(), inner.clone());
                    }
                }
            }
        }
        parts.retain(|_, document| !document.is_empty());
    }

    fn insert_value(
        &self,
        form: StorageForm,
        parts: &mut BTreeMap<String, Map<String, Value>>,
        id: AttributeId,
        value: &Value,
    ) {
        let key = attr_key(id);
        let group = self.group_of(id);
        if self.is_localized(id) && form != StorageForm::Document {
            let Some(map) = value.as_object() else {
                let part = self.part_name(form, None, group);
                parts.entry(part).or_default().insert(key, value.clone());
                return;
            };
            for (language, inner) in map {
                let part = self.part_name(form, Some(language), group);
                parts
                    .entry(part)
                    .or_default()
                    .insert(key.clone(), inner.clone());
            }
            return;
        }
        let part = self.part_name(form, None, group);
        parts.entry(part).or_default().insert(key, value.clone());
    }
}

/// Rebuilds object values from part documents. Part names carry the language,
/// so this does not need the definition set.
pub fn join_parts(
    form: StorageForm,
    parts: &BTreeMap<String, Value>,
) -> Result<BTreeMap<AttributeId, Value>, String> {
    let mut values = BTreeMap::new();
    for (part, document) in parts {
        let Some(document) = document.as_object() else {
            return Err(format!("part {part} is not a JSON object"));
        };
        let language = language_of(form, part);
        for (key, value) in document {
            let id = parse_attribute(key)?;
            if let Some(language) = language {
                let slot = values
                    .entry(id)
                    .or_insert_with(|| Value::Object(Map::new()));
                if !slot.is_object() {
                    *slot = Value::Object(Map::new());
                }
                let Some(map) = slot.as_object_mut() else {
                    continue;
                };
                map.insert(language.to_owned(), value.clone());
            } else {
                values.insert(id, value.clone());
            }
        }
    }
    Ok(values)
}

pub fn language_of(form: StorageForm, part: &str) -> Option<&str> {
    match form {
        StorageForm::Document => None,
        StorageForm::PerLanguage => {
            if part.is_empty() {
                None
            } else {
                Some(part)
            }
        }
        StorageForm::PerLanguageGroup => {
            if part.starts_with('#') {
                None
            } else {
                part.split_once('#').map(|(language, _)| language)
            }
        }
    }
}

pub fn attr_key(id: AttributeId) -> String {
    id.as_uuid().to_string()
}

fn parse_attribute(key: &str) -> Result<AttributeId, String> {
    Uuid::parse_str(key)
        .map(AttributeId::from_uuid)
        .map_err(|error| format!("attribute id {key}: {error}"))
}

/// Object-shaped diff: `{attr: {"before": v, "after": v}}`.
///
/// Language patches include only the languages the patch changes. `before` is
/// omitted when the attribute (or that language) was absent.
pub fn diff_from_patch(
    ctx: &SplitContext,
    form: StorageForm,
    before_parts: &BTreeMap<String, Map<String, Value>>,
    patch: &ValuePatch,
) -> Value {
    let mut diff = Map::new();
    for (id, change) in &patch.attributes {
        let key = attr_key(*id);
        let (before, after) = match change {
            AttributePatch::Replace { value } => {
                (reconstruct(ctx, form, before_parts, *id), value.clone())
            }
            AttributePatch::Languages { values } => {
                let mut before_map = Map::new();
                let mut after_map = Map::new();
                for (language, inner) in values {
                    if let Some(existing) =
                        before_language(ctx, form, before_parts, *id, language.as_str())
                    {
                        before_map.insert(language.as_str().to_owned(), existing);
                    }
                    after_map.insert(language.as_str().to_owned(), inner.clone());
                }
                let before = (!before_map.is_empty()).then_some(Value::Object(before_map));
                (before, Value::Object(after_map))
            }
        };
        diff.insert(key, change_entry(before, after));
    }
    Value::Object(diff)
}

pub fn diff_created(values: &BTreeMap<AttributeId, Value>) -> Value {
    let mut diff = Map::new();
    for (id, value) in values {
        diff.insert(attr_key(*id), change_entry(None, value.clone()));
    }
    Value::Object(diff)
}

fn change_entry(before: Option<Value>, after: Value) -> Value {
    let mut entry = Map::new();
    if let Some(before) = before {
        entry.insert("before".to_owned(), before);
    }
    entry.insert("after".to_owned(), after);
    Value::Object(entry)
}

fn reconstruct(
    ctx: &SplitContext,
    form: StorageForm,
    parts: &BTreeMap<String, Map<String, Value>>,
    id: AttributeId,
) -> Option<Value> {
    let key = attr_key(id);
    if form == StorageForm::Document || !ctx.is_localized(id) {
        let part = ctx.part_name(form, None, ctx.group_of(id));
        return parts.get(&part)?.get(&key).cloned();
    }
    let mut map = Map::new();
    for (part, document) in parts {
        let Some(language) = language_of(form, part) else {
            continue;
        };
        if let Some(inner) = document.get(&key) {
            map.insert(language.to_owned(), inner.clone());
        }
    }
    (!map.is_empty()).then_some(Value::Object(map))
}

fn before_language(
    ctx: &SplitContext,
    form: StorageForm,
    parts: &BTreeMap<String, Map<String, Value>>,
    id: AttributeId,
    language: &str,
) -> Option<Value> {
    let key = attr_key(id);
    if form == StorageForm::Document {
        return parts.get("")?.get(&key)?.get(language).cloned();
    }
    let part = ctx.part_name(form, Some(language), ctx.group_of(id));
    parts.get(&part)?.get(&key).cloned()
}

pub fn documents_from_rows(
    rows: impl IntoIterator<Item = (String, Value)>,
) -> Result<BTreeMap<String, Map<String, Value>>, String> {
    let mut parts = BTreeMap::new();
    for (part, value) in rows {
        let Some(document) = value.as_object().cloned() else {
            return Err(format!("part {part} is not a JSON object"));
        };
        parts.insert(part, document);
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use openpim_model::apply_value_patch;
    use openpim_synth::{EditKind, Tier, definitions, edits, objects};

    use super::*;

    #[test]
    fn heavy_definitions_land_near_twenty_groups() {
        let defs = definitions(Tier::Heavy, 1);
        let ctx = SplitContext::new(&defs);
        // 2000 attributes plus the unused one, in blocks of 100.
        assert_eq!(ctx.group_count(), 21);
    }

    #[test]
    fn split_join_roundtrips_every_form() {
        let defs = definitions(Tier::Light, 3);
        let ctx = SplitContext::new(&defs);
        let sample = objects(&defs, Tier::Light, 3, 2);
        for form in StorageForm::ALL {
            for object in &sample {
                let parts = ctx.split(form, &object.values);
                let encoded = parts
                    .iter()
                    .map(|(part, document)| (part.clone(), Value::Object(document.clone())))
                    .collect();
                let joined = ctx.join(form, &encoded).expect("join");
                assert_eq!(joined, object.values, "{}", form.name());
            }
        }
    }

    #[test]
    fn patch_touches_one_part_per_language_edit() {
        let defs = definitions(Tier::Light, 1);
        let ctx = SplitContext::new(&defs);
        let edit = edits(&defs, Tier::Light, 1, EditKind::OneLanguage, 0..1)
            .next()
            .unwrap();
        for form in [StorageForm::PerLanguage, StorageForm::PerLanguageGroup] {
            let parts = ctx.affected_parts(form, &edit.patch);
            assert_eq!(parts.len(), 1, "{}", form.name());
        }
        assert_eq!(
            ctx.affected_parts(StorageForm::Document, &edit.patch),
            vec![String::new()]
        );
    }

    #[test]
    fn applying_a_patch_matches_the_object_patch() {
        let defs = definitions(Tier::Medium, 2);
        let ctx = SplitContext::new(&defs);
        let object = objects(&defs, Tier::Medium, 2, 1).pop().unwrap();
        for kind in EditKind::ALL {
            let edit = edits(&defs, Tier::Medium, 2, kind, 0..1).next().unwrap();
            for form in StorageForm::ALL {
                let mut parts = ctx.split(form, &object.values);
                let affected = ctx.affected_parts(form, &edit.patch);
                parts.retain(|part, _| affected.iter().any(|name| name == part));
                ctx.apply_patch(form, &mut parts, &edit.patch);
                let mut expected = object.values.clone();
                apply_value_patch(&mut expected, &edit.patch);
                let mut stored = ctx.split(form, &expected);
                stored.retain(|part, _| affected.iter().any(|name| name == part));
                assert_eq!(parts, stored, "{} {}", form.name(), kind.name());
            }
        }
    }
}
