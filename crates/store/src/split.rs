use std::collections::{BTreeMap, BTreeSet, HashMap};

use braian_model::{AttributeId, AttributePatch, DefinitionSet, ValuePatch};
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
    /// Block of ~100 attributes. An attribute keeps the group it was first given.
    group: HashMap<AttributeId, usize>,
    languages: Vec<String>,
}

pub(crate) const GROUP_SIZE: usize = 100;

impl SplitContext {
    /// Groups from attribute order.
    ///
    /// This is the rule databases used before `attribute_parts` existed, and the
    /// rule [`Self::assign_groups`] reproduces when nothing is assigned yet.
    pub fn sorted_groups(defs: &DefinitionSet) -> HashMap<AttributeId, usize> {
        let mut order: Vec<_> = defs.attributes.iter().collect();
        order.sort_by(|left, right| {
            left.key
                .cmp(&right.key)
                .then(left.id.as_uuid().cmp(&right.id.as_uuid()))
        });
        order
            .into_iter()
            .enumerate()
            .map(|(index, attribute)| (attribute.id, index / GROUP_SIZE))
            .collect()
    }

    /// Keeps every assigned group and fills the lowest group that still has room.
    ///
    /// New attributes are ordered by key, so an empty assignment matches
    /// [`Self::sorted_groups`]. An attribute that already has a group never moves.
    pub fn assign_groups(
        defs: &DefinitionSet,
        existing: &HashMap<AttributeId, usize>,
    ) -> HashMap<AttributeId, usize> {
        let mut counts: BTreeMap<usize, usize> = BTreeMap::new();
        let mut assigned = HashMap::new();
        for attribute in &defs.attributes {
            if let Some(group) = existing.get(&attribute.id).copied() {
                assigned.insert(attribute.id, group);
                *counts.entry(group).or_default() += 1;
            }
        }
        let mut fresh: Vec<_> = defs
            .attributes
            .iter()
            .filter(|attribute| !assigned.contains_key(&attribute.id))
            .collect();
        fresh.sort_by(|left, right| {
            left.key
                .cmp(&right.key)
                .then(left.id.as_uuid().cmp(&right.id.as_uuid()))
        });
        for attribute in fresh {
            let group = counts
                .iter()
                .find(|(_, count)| **count < GROUP_SIZE)
                .map(|(group, _)| *group)
                .unwrap_or_else(|| {
                    counts
                        .keys()
                        .next_back()
                        .map(|group| group + 1)
                        .unwrap_or(0)
                });
            *counts.entry(group).or_default() += 1;
            assigned.insert(attribute.id, group);
        }
        assigned
    }

    /// Split context using [`Self::sorted_groups`].
    #[cfg(test)]
    pub fn new(defs: &DefinitionSet) -> Self {
        Self::with_groups(defs, &Self::sorted_groups(defs))
    }

    pub fn with_groups(defs: &DefinitionSet, groups: &HashMap<AttributeId, usize>) -> Self {
        let language_dependent = defs
            .attributes
            .iter()
            .map(|attribute| (attribute.id, attribute.language_dependent))
            .collect();
        Self {
            language_dependent,
            group: groups.clone(),
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

    pub(crate) fn group_of(&self, id: AttributeId) -> usize {
        self.group.get(&id).copied().unwrap_or(0)
    }

    pub(crate) fn is_localized(&self, id: AttributeId) -> bool {
        self.language_dependent.get(&id).copied().unwrap_or(false)
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
                AttributePatch::Remove if self.is_localized(*id) => {
                    parts.insert(self.part_name(form, None, group));
                    for language in &self.languages {
                        parts.insert(self.part_name(form, Some(language), group));
                    }
                }
                AttributePatch::Remove => {
                    parts.insert(self.part_name(form, None, group));
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

    /// Same parts as [`split`](Self::split), borrowing every value. The bulk
    /// loader serializes these straight into COPY, so no value tree is cloned.
    pub fn split_ref<'a>(
        &self,
        form: StorageForm,
        values: &'a BTreeMap<AttributeId, Value>,
    ) -> BTreeMap<String, BTreeMap<String, &'a Value>> {
        let mut parts: BTreeMap<String, BTreeMap<String, &'a Value>> = BTreeMap::new();
        for (id, value) in values {
            let key = attr_key(*id);
            let group = self.group_of(*id);
            if self.is_localized(*id)
                && form != StorageForm::Document
                && let Some(map) = value.as_object()
            {
                for (language, inner) in map {
                    let part = self.part_name(form, Some(language), group);
                    parts.entry(part).or_default().insert(key.clone(), inner);
                }
                continue;
            }
            let part = self.part_name(form, None, group);
            parts.entry(part).or_default().insert(key, value);
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
    ///
    /// A replace also drops the attribute from every other loaded part in its
    /// group. That clears a copy left behind when `language_dependent` flips.
    /// A language patch only drops a leftover in the language-independent part,
    /// so the other languages stay where they are.
    pub fn apply_patch(
        &self,
        form: StorageForm,
        parts: &mut BTreeMap<String, Map<String, Value>>,
        patch: &ValuePatch,
    ) {
        for (id, change) in &patch.attributes {
            let key = attr_key(*id);
            let group = self.group_of(*id);
            match change {
                AttributePatch::Replace { value }
                    if self.is_localized(*id) && form != StorageForm::Document =>
                {
                    strip_attribute(form, parts, group, &key);
                    self.insert_value(form, parts, *id, value);
                }
                AttributePatch::Replace { value } => {
                    strip_attribute(form, parts, group, &key);
                    let part = self.part_name(form, None, group);
                    parts.entry(part).or_default().insert(key, value.clone());
                }
                AttributePatch::Remove => {
                    strip_attribute(form, parts, group, &key);
                }
                AttributePatch::Languages { values } if form == StorageForm::Document => {
                    let document = parts.entry(String::new()).or_default();
                    let mut empty = false;
                    {
                        let slot = document
                            .entry(key.clone())
                            .or_insert_with(|| Value::Object(Map::new()));
                        if !slot.is_object() {
                            *slot = Value::Object(Map::new());
                        }
                        if let Some(map) = slot.as_object_mut() {
                            for (language, inner) in values {
                                match inner {
                                    Some(inner) => {
                                        map.insert(language.as_str().to_owned(), inner.clone());
                                    }
                                    None => {
                                        map.remove(language.as_str());
                                    }
                                }
                            }
                            empty = map.is_empty();
                        }
                    }
                    if empty {
                        document.remove(&key);
                    }
                }
                AttributePatch::Languages { values } => {
                    if self.is_localized(*id) {
                        let independent = self.part_name(form, None, group);
                        if let Some(document) = parts.get_mut(&independent) {
                            document.remove(&key);
                        }
                    }
                    for (language, inner) in values {
                        let part = self.part_name(form, Some(language.as_str()), group);
                        match inner {
                            Some(inner) => {
                                parts
                                    .entry(part)
                                    .or_default()
                                    .insert(key.clone(), inner.clone());
                            }
                            None => {
                                if let Some(document) = parts.get_mut(&part) {
                                    document.remove(&key);
                                }
                            }
                        }
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

/// Parts an update has to read so a stale copy of the same attribute can be cleared.
///
/// `destinations` are the parts the patch writes. The key lists name attributes
/// whose opposite shape should be loaded when that part still holds the key:
/// a localized attribute stored in the language-independent part, or a plain
/// attribute stored in a language part. Parts that do not change are not rewritten.
#[derive(Debug, Default)]
pub(crate) struct StaleParts {
    pub destinations: Vec<String>,
    pub localized_groups: Vec<String>,
    pub localized_keys: Vec<String>,
    pub plain_groups: Vec<String>,
    pub plain_keys: Vec<String>,
}

impl SplitContext {
    pub(crate) fn stale_parts(&self, form: StorageForm, patch: &ValuePatch) -> StaleParts {
        let mut stale = StaleParts {
            destinations: self.affected_parts(form, patch),
            ..StaleParts::default()
        };
        for id in patch.attributes.keys() {
            let group = self.group_of(*id).to_string();
            let key = attr_key(*id);
            if self.is_localized(*id) {
                stale.localized_groups.push(group);
                stale.localized_keys.push(key);
            } else {
                stale.plain_groups.push(group);
                stale.plain_keys.push(key);
            }
        }
        stale
    }
}

fn strip_attribute(
    form: StorageForm,
    parts: &mut BTreeMap<String, Map<String, Value>>,
    group: usize,
    key: &str,
) {
    for (part, document) in parts.iter_mut() {
        if part_in_group(form, part, group) {
            document.remove(key);
        }
    }
}

fn part_in_group(form: StorageForm, part: &str, group: usize) -> bool {
    match form {
        StorageForm::Document | StorageForm::PerLanguage => true,
        StorageForm::PerLanguageGroup => part_group(part) == Some(group),
    }
}

pub(crate) fn part_group(part: &str) -> Option<usize> {
    let (_, suffix) = part.rsplit_once('#')?;
    suffix.parse().ok()
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
            AttributePatch::Replace { value }
                if ctx.is_localized(*id) && value.as_object().is_some() =>
            {
                language_replace_diff(ctx, form, before_parts, *id, value)
            }
            AttributePatch::Replace { value } => {
                (reconstruct(ctx, form, before_parts, *id), value.clone())
            }
            AttributePatch::Remove => (reconstruct(ctx, form, before_parts, *id), Value::Null),
            AttributePatch::Languages { values } => {
                let mut before_map = Map::new();
                let mut after_map = Map::new();
                for (language, inner) in values {
                    if let Some(existing) =
                        before_language(ctx, form, before_parts, *id, language.as_str())
                    {
                        before_map.insert(language.as_str().to_owned(), existing);
                    }
                    let after = inner.clone().unwrap_or(Value::Null);
                    after_map.insert(language.as_str().to_owned(), after);
                }
                let before = (!before_map.is_empty()).then_some(Value::Object(before_map));
                (before, Value::Object(after_map))
            }
        };
        diff.insert(key, change_entry(before, after));
    }
    Value::Object(diff)
}

/// The create diff as a borrowed, serialize-only view: `{attr: {"after": v}}`
/// plus `$groups` when the object is placed. Same JSON as
/// [`diff_created`] followed by attaching groups, without cloning the values.
#[derive(Debug)]
pub struct CreatedDiff<'a> {
    pub values: &'a BTreeMap<AttributeId, Value>,
    pub groups: &'a BTreeMap<braian_model::StructureId, braian_model::GroupId>,
}

struct After<T>(T);

impl<T: serde::Serialize> serde::Serialize for After<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("after", &self.0)?;
        map.end()
    }
}

struct Groups<'a>(&'a BTreeMap<braian_model::StructureId, braian_model::GroupId>);

impl serde::Serialize for Groups<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (structure, group) in self.0 {
            map.serialize_entry(
                &structure.as_uuid().to_string(),
                &After(group.as_uuid().to_string()),
            )?;
        }
        map.end()
    }
}

impl serde::Serialize for CreatedDiff<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let extra = usize::from(!self.groups.is_empty());
        let mut map = serializer.serialize_map(Some(self.values.len() + extra))?;
        for (id, value) in self.values {
            map.serialize_entry(&attr_key(*id), &After(value))?;
        }
        if !self.groups.is_empty() {
            map.serialize_entry("$groups", &Groups(self.groups))?;
        }
        map.end()
    }
}

pub fn diff_created(values: &BTreeMap<AttributeId, Value>) -> Value {
    let mut diff = Map::new();
    for (id, value) in values {
        diff.insert(attr_key(*id), change_entry(None, value.clone()));
    }
    Value::Object(diff)
}

/// A localized replace records only the languages that changed. A language that
/// disappeared is `after: null`, so a later conflict check can see it.
fn language_replace_diff(
    ctx: &SplitContext,
    form: StorageForm,
    before_parts: &BTreeMap<String, Map<String, Value>>,
    id: AttributeId,
    value: &Value,
) -> (Option<Value>, Value) {
    let after_map = value.as_object().cloned().unwrap_or_default();
    let before_value = reconstruct(ctx, form, before_parts, id);
    let before_map = before_value
        .as_ref()
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut languages = BTreeSet::new();
    languages.extend(before_map.keys().cloned());
    languages.extend(after_map.keys().cloned());
    let mut before_out = Map::new();
    let mut after_out = Map::new();
    for language in languages {
        let old = before_map.get(&language).cloned();
        let new = after_map.get(&language).cloned();
        if old == new {
            continue;
        }
        if let Some(old) = old {
            before_out.insert(language.clone(), old);
        }
        after_out.insert(language, new.unwrap_or(Value::Null));
    }
    let before = (!before_out.is_empty()).then_some(Value::Object(before_out));
    (before, Value::Object(after_out))
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
    use braian_model::apply_value_patch;
    use braian_synth::{EditKind, Tier, definitions, edits, objects};

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
    fn borrowed_bulk_encoding_matches_the_owned_one() {
        let defs = definitions(Tier::Medium, 5);
        let ctx = SplitContext::new(&defs);
        for object in &objects(&defs, Tier::Medium, 5, 3) {
            for form in StorageForm::ALL {
                let owned = serde_json::to_value(ctx.split(form, &object.values)).unwrap();
                let borrowed = serde_json::to_value(ctx.split_ref(form, &object.values)).unwrap();
                assert_eq!(owned, borrowed, "{}", form.name());
            }
            let mut expected = diff_created(&object.values);
            let mut placed = Map::new();
            for (structure, group) in &object.groups {
                placed.insert(
                    structure.as_uuid().to_string(),
                    serde_json::json!({ "after": group.as_uuid().to_string() }),
                );
            }
            if !placed.is_empty() {
                expected
                    .as_object_mut()
                    .unwrap()
                    .insert("$groups".to_owned(), Value::Object(placed));
            }
            let view = CreatedDiff {
                values: &object.values,
                groups: &object.groups,
            };
            assert_eq!(serde_json::to_value(&view).unwrap(), expected);
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

    #[test]
    fn empty_assignment_matches_the_sorted_groups() {
        let defs = definitions(Tier::Heavy, 4);
        let assigned = SplitContext::assign_groups(&defs, &HashMap::new());
        assert_eq!(assigned, SplitContext::sorted_groups(&defs));
    }

    #[test]
    fn new_attributes_fill_open_groups_without_moving_old_ones() {
        let defs = definitions(Tier::Light, 5);
        let original = SplitContext::sorted_groups(&defs);
        let mut expanded = defs.clone();
        for index in 0..150 {
            expanded.attributes.push(braian_model::Attribute {
                id: braian_model::AttributeId::from_uuid(uuid::Uuid::from_u128(index + 1)),
                key: format!("0{index:04}"),
                labels: Default::default(),
                language_dependent: false,
                multiple: false,
                required: false,
                kind: braian_model::AttributeKind::Text {
                    max_length: None,
                    form: braian_model::TextForm::Short,
                },
                index: braian_model::IndexFlags::default(),
            });
        }
        let assigned = SplitContext::assign_groups(&expanded, &original);
        for (id, group) in &original {
            assert_eq!(assigned.get(id), Some(group));
        }
        assert_ne!(assigned, SplitContext::sorted_groups(&expanded));
    }

    #[test]
    fn language_patch_drops_a_stale_independent_copy() {
        let defs = definitions(Tier::Light, 1);
        let ctx = SplitContext::new(&defs);
        let object = objects(&defs, Tier::Light, 1, 1).pop().unwrap();
        let edit = edits(&defs, Tier::Light, 1, EditKind::OneLanguage, 0..1)
            .next()
            .unwrap();
        let id = *edit.patch.attributes.keys().next().unwrap();
        let mut parts = ctx.split(StorageForm::PerLanguageGroup, &object.values);
        let group = ctx.group_of(id);
        let independent = ctx.part_name(StorageForm::PerLanguageGroup, None, group);
        let key = attr_key(id);
        parts
            .entry(independent.clone())
            .or_default()
            .insert(key.clone(), Value::String("stale".into()));
        ctx.apply_patch(StorageForm::PerLanguageGroup, &mut parts, &edit.patch);
        let left = parts
            .get(&independent)
            .and_then(|document| document.get(&key));
        assert_eq!(left, None);
    }
}
