use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use braian_model::{
    Attribute, AttributeId, AttributeKind, ClassId, DefinitionSet, GroupId, StructureId, TypeId,
    UnitFamilyId,
};

use crate::validate::ValueIndex;

/// A definition set that failed to compile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Error)]
#[error("{message}")]
pub struct DefinitionError {
    pub message: String,
}

/// One attribute that applies to an object, and every place it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveAttribute {
    pub id: AttributeId,
    pub sources: Vec<AttributeSource>,
}

/// A type or a structure node that contributes an attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttributeSource {
    Type { key: String },
    Group { structure: String, group: String },
}

#[derive(Debug, Clone)]
pub(crate) struct TypeInfo {
    pub(crate) key_attribute: AttributeId,
    pub(crate) attributes: HashSet<AttributeId>,
}

#[derive(Debug, Clone)]
pub(crate) struct GroupInfo {
    pub(crate) structure_id: StructureId,
    /// Attributes contributed by this group's classes and every ancestor group's classes.
    pub(crate) class_attributes: HashSet<AttributeId>,
}

#[derive(Debug, Clone)]
struct UnitInfo {
    factors: HashMap<String, rust_decimal::Decimal>,
}

/// Indexes built from a [`DefinitionSet`].
///
/// Compilation is the only place that walks parent links. Validation then
/// looks up the applicable attributes for the object's groups.
#[derive(Debug, Clone)]
pub struct CompiledModel {
    defs: DefinitionSet,
    attributes: HashMap<AttributeId, usize>,
    attribute_keys: HashMap<String, AttributeId>,
    types: HashMap<TypeId, TypeInfo>,
    type_keys: HashMap<String, TypeId>,
    structure_keys: HashMap<String, StructureId>,
    groups: HashMap<GroupId, GroupInfo>,
    group_keys: HashMap<(String, String), GroupId>,
    languages: HashSet<braian_model::LanguageTag>,
    units: HashMap<UnitFamilyId, UnitInfo>,
    values: ValueIndex,
}

impl CompiledModel {
    pub fn compile(defs: &DefinitionSet) -> Result<Self, Vec<DefinitionError>> {
        let mut errors = Vec::new();
        check(defs, &mut errors);
        if !errors.is_empty() {
            errors.sort_by(|left, right| left.message.cmp(&right.message));
            errors.dedup();
            return Err(errors);
        }
        Ok(index(defs))
    }

    pub fn languages(&self) -> &[braian_model::LanguageTag] {
        &self.defs.languages
    }

    pub fn attribute(&self, id: AttributeId) -> Option<&Attribute> {
        self.attributes
            .get(&id)
            .map(|&index| &self.defs.attributes[index])
    }

    pub fn attribute_by_key(&self, key: &str) -> Option<&Attribute> {
        self.attribute_keys
            .get(key)
            .and_then(|id| self.attribute(*id))
    }

    pub fn attributes(&self) -> impl Iterator<Item = &Attribute> {
        self.defs.attributes.iter()
    }

    pub fn type_id(&self, key: &str) -> Option<TypeId> {
        self.type_keys.get(key).copied()
    }

    pub fn structure_id(&self, key: &str) -> Option<StructureId> {
        self.structure_keys.get(key).copied()
    }

    pub fn group_id(&self, structure_key: &str, group_key: &str) -> Option<GroupId> {
        self.group_keys
            .get(&(structure_key.to_owned(), group_key.to_owned()))
            .copied()
    }

    pub fn key_attribute(&self, type_id: TypeId) -> Option<AttributeId> {
        self.types.get(&type_id).map(|info| info.key_attribute)
    }

    /// Attributes that apply to an object of this type placed in these groups.
    ///
    /// Returns `None` when the type or a group is unknown, or when a group
    /// does not belong to the structure it was filed under.
    pub fn applicable_ids(
        &self,
        type_id: TypeId,
        groups: &std::collections::BTreeMap<StructureId, GroupId>,
    ) -> Option<Vec<AttributeId>> {
        let info = self.types.get(&type_id)?;
        let mut applicable = info.attributes.clone();
        for (structure_id, group_id) in groups {
            let group = self.groups.get(group_id)?;
            if group.structure_id != *structure_id {
                return None;
            }
            applicable.extend(group.class_attributes.iter().copied());
        }
        let mut ids: Vec<_> = applicable.into_iter().collect();
        ids.sort();
        Some(ids)
    }

    /// Attributes an object of `type_id` gets from its type and from every
    /// group it belongs to, including ancestor groups.
    ///
    /// The same attribute id is returned once. `sources` names each place that
    /// contributed it: the type, and each group that defines it.
    pub fn effective_attributes(
        &self,
        type_id: TypeId,
        membership: &std::collections::BTreeMap<StructureId, GroupId>,
    ) -> Option<Vec<EffectiveAttribute>> {
        let object_type = self.defs.types.iter().find(|item| item.id == type_id)?;
        let mut order = Vec::new();
        let mut sources: HashMap<AttributeId, Vec<AttributeSource>> = HashMap::new();
        let mut push = |id: AttributeId, source: AttributeSource| {
            let entry = sources.entry(id).or_insert_with(|| {
                order.push(id);
                Vec::new()
            });
            if !entry.contains(&source) {
                entry.push(source);
            }
        };
        for id in &object_type.attributes {
            push(
                *id,
                AttributeSource::Type {
                    key: object_type.key.clone(),
                },
            );
        }
        for (structure_id, group_id) in membership {
            let structure = self
                .defs
                .structures
                .iter()
                .find(|item| item.id == *structure_id)?;
            let mut parent_of = HashMap::new();
            let mut key_of = HashMap::new();
            for group in &structure.groups {
                parent_of.insert(group.id, group.parent);
                key_of.insert(group.id, group.key.clone());
            }
            if !key_of.contains_key(group_id) {
                return None;
            }
            let mut defined: HashMap<GroupId, Vec<AttributeId>> = HashMap::new();
            for group in &structure.groups {
                defined
                    .entry(group.id)
                    .or_default()
                    .extend(group.attributes.iter().copied());
            }
            for class in &structure.classes {
                defined
                    .entry(class.group)
                    .or_default()
                    .extend(class.attributes.iter().copied());
            }
            let mut chain = Vec::new();
            let mut current = Some(*group_id);
            let mut seen = HashSet::new();
            while let Some(id) = current {
                if !seen.insert(id) {
                    break;
                }
                chain.push(id);
                current = parent_of.get(&id).copied().flatten();
            }
            chain.reverse();
            for id in chain {
                let Some(group_key) = key_of.get(&id) else {
                    continue;
                };
                let source = AttributeSource::Group {
                    structure: structure.key.clone(),
                    group: group_key.clone(),
                };
                for attribute_id in defined.get(&id).into_iter().flatten() {
                    push(*attribute_id, source.clone());
                }
            }
        }
        Some(
            order
                .into_iter()
                .filter_map(|id| {
                    sources
                        .remove(&id)
                        .map(|sources| EffectiveAttribute { id, sources })
                })
                .collect(),
        )
    }

    pub(crate) fn is_known_language(&self, tag: &braian_model::LanguageTag) -> bool {
        self.languages.contains(tag)
    }

    pub(crate) fn type_info(&self, type_id: TypeId) -> Option<&TypeInfo> {
        self.types.get(&type_id)
    }

    pub(crate) fn group_info(&self, group_id: GroupId) -> Option<&GroupInfo> {
        self.groups.get(&group_id)
    }

    pub(crate) fn unit_factor(
        &self,
        family: UnitFamilyId,
        unit: &str,
    ) -> Option<rust_decimal::Decimal> {
        self.units.get(&family)?.factors.get(unit).copied()
    }

    pub(crate) fn has_structure(&self, id: StructureId) -> bool {
        self.structure_keys
            .values()
            .any(|structure| *structure == id)
    }

    pub(crate) fn value_index(&self) -> &ValueIndex {
        &self.values
    }
}

fn index(defs: &DefinitionSet) -> CompiledModel {
    let attributes = defs
        .attributes
        .iter()
        .enumerate()
        .map(|(index, attribute)| (attribute.id, index))
        .collect::<HashMap<_, _>>();
    let attribute_keys = defs
        .attributes
        .iter()
        .map(|attribute| (attribute.key.clone(), attribute.id))
        .collect::<HashMap<_, _>>();

    let mut direct: HashMap<GroupId, HashSet<AttributeId>> = HashMap::new();
    let mut parent: HashMap<GroupId, Option<GroupId>> = HashMap::new();
    let mut group_structure: HashMap<GroupId, StructureId> = HashMap::new();
    let mut structure_keys = HashMap::new();
    let mut group_keys = HashMap::new();
    for structure in &defs.structures {
        structure_keys.insert(structure.key.clone(), structure.id);
        for group in &structure.groups {
            parent.insert(group.id, group.parent);
            group_structure.insert(group.id, structure.id);
            group_keys.insert((structure.key.clone(), group.key.clone()), group.id);
            direct
                .entry(group.id)
                .or_default()
                .extend(group.attributes.iter().copied());
        }
        let mut class_attrs = HashMap::new();
        let mut class_parent = HashMap::new();
        for class in &structure.classes {
            class_attrs.insert(class.id, class.attributes.clone());
            class_parent.insert(class.id, class.parent);
        }
        for class in &structure.classes {
            direct
                .entry(class.group)
                .or_default()
                .extend(effective_attributes(class.id, &class_attrs, &class_parent));
        }
    }

    let mut groups = HashMap::new();
    for (group_id, structure_id) in &group_structure {
        let mut class_attributes = HashSet::new();
        let mut current = Some(*group_id);
        let mut seen = HashSet::new();
        while let Some(id) = current {
            if !seen.insert(id) {
                break;
            }
            if let Some(attributes) = direct.get(&id) {
                class_attributes.extend(attributes.iter().copied());
            }
            current = parent.get(&id).copied().flatten();
        }
        groups.insert(
            *group_id,
            GroupInfo {
                structure_id: *structure_id,
                class_attributes,
            },
        );
    }

    let types = defs
        .types
        .iter()
        .map(|object_type| {
            (
                object_type.id,
                TypeInfo {
                    key_attribute: object_type.key_attribute,
                    attributes: object_type.attributes.iter().copied().collect(),
                },
            )
        })
        .collect();
    let type_keys = defs
        .types
        .iter()
        .map(|object_type| (object_type.key.clone(), object_type.id))
        .collect();

    let units = defs
        .unit_families
        .iter()
        .map(|family| {
            let factors = family
                .units
                .iter()
                .map(|unit| (unit.code.clone(), unit.factor))
                .collect::<HashMap<_, _>>();
            (family.id, UnitInfo { factors })
        })
        .collect();

    let languages = defs.languages.iter().cloned().collect();
    let values = ValueIndex::build(defs);

    CompiledModel {
        defs: defs.clone(),
        attributes,
        attribute_keys,
        types,
        type_keys,
        structure_keys,
        groups,
        group_keys,
        languages,
        units,
        values,
    }
}

fn check(defs: &DefinitionSet, errors: &mut Vec<DefinitionError>) {
    let mut ids: HashMap<braian_model::Uuid, &'static str> = HashMap::new();
    let mut remember =
        |id: braian_model::Uuid, kind: &'static str, errors: &mut Vec<DefinitionError>| {
            if let Some(previous) = ids.insert(id, kind) {
                errors.push(error(format!("{kind} id {id} duplicates a {previous} id")));
            }
        };

    let mut languages = HashSet::new();
    for language in &defs.languages {
        if !languages.insert(language.clone()) {
            errors.push(error(format!("duplicate language {language}")));
        }
    }

    let mut list_keys = HashSet::new();
    for list in &defs.value_lists {
        remember(list.id.as_uuid(), "value list", errors);
        unique_key(&mut list_keys, &list.key, "value list", errors);
        let mut codes = HashSet::new();
        for option in &list.options {
            if option.code.is_empty() || !codes.insert(option.code.clone()) {
                errors.push(error(format!(
                    "value list {} has an empty or duplicate code",
                    list.key
                )));
            }
        }
    }

    let mut family_keys = HashSet::new();
    let mut family_ids = HashSet::new();
    for family in &defs.unit_families {
        remember(family.id.as_uuid(), "unit family", errors);
        unique_key(&mut family_keys, &family.key, "unit family", errors);
        family_ids.insert(family.id);
        let mut codes = HashSet::new();
        let mut base_factors = 0usize;
        for unit in &family.units {
            if unit.code.is_empty() {
                errors.push(error(format!(
                    "unit family {} has an empty unit code",
                    family.key
                )));
            } else if !codes.insert(unit.code.clone()) {
                errors.push(error(format!(
                    "unit family {} has duplicate unit {}",
                    family.key, unit.code
                )));
            }
            if unit.factor <= rust_decimal::Decimal::ZERO {
                errors.push(error(format!(
                    "unit {} in family {} must have a positive factor",
                    unit.code, family.key
                )));
            }
            if unit.factor == rust_decimal::Decimal::ONE {
                base_factors += 1;
            }
        }
        let base = family
            .units
            .iter()
            .find(|unit| unit.code == family.base_unit);
        if base.is_none()
            || base_factors != 1
            || base.is_some_and(|unit| unit.factor != rust_decimal::Decimal::ONE)
        {
            errors.push(error(format!(
                "unit family {} must have exactly one base unit {} with factor 1",
                family.key, family.base_unit
            )));
        }
    }

    let mut attribute_keys = HashSet::new();
    let mut attribute_ids = HashSet::new();
    for attribute in &defs.attributes {
        remember(attribute.id.as_uuid(), "attribute", errors);
        unique_key(&mut attribute_keys, &attribute.key, "attribute", errors);
        attribute_ids.insert(attribute.id);
        if attribute.language_dependent && defs.languages.is_empty() {
            errors.push(error(format!(
                "language-dependent attribute {} needs at least one language",
                attribute.key
            )));
        }
        let unit_family = match &attribute.kind {
            AttributeKind::Number { unit_family, .. } | AttributeKind::Range { unit_family } => {
                *unit_family
            }
            _ => None,
        };
        if let Some(family) = unit_family
            && !family_ids.contains(&family)
        {
            errors.push(error(format!(
                "attribute {} references unknown unit family {family}",
                attribute.key
            )));
        }
        if let AttributeKind::Number { min, max, .. } = &attribute.kind
            && let (Some(min), Some(max)) = (min, max)
            && min > max
        {
            errors.push(error(format!(
                "attribute {} has min greater than max",
                attribute.key
            )));
        }
        if let AttributeKind::ValueList { options, list } = &attribute.kind {
            if let Some(list_key) = list {
                if !defs.value_lists.iter().any(|item| item.key == *list_key) {
                    errors.push(error(format!(
                        "attribute {} references unknown value list {list_key}",
                        attribute.key
                    )));
                }
                if !options.is_empty() {
                    errors.push(error(format!(
                        "attribute {} uses shared list {list_key} and cannot also carry options",
                        attribute.key
                    )));
                }
            } else {
                let mut codes = HashSet::new();
                for option in options {
                    if option.code.is_empty() {
                        errors.push(error(format!(
                            "attribute {} has an empty value-list code",
                            attribute.key
                        )));
                    } else if !codes.insert(option.code.clone()) {
                        errors.push(error(format!(
                            "attribute {} has duplicate value-list code {}",
                            attribute.key, option.code
                        )));
                    }
                }
            }
        }
        if let AttributeKind::ObjectRef {
            type_key: Some(type_key),
        } = &attribute.kind
            && !defs
                .types
                .iter()
                .any(|object_type| object_type.key == *type_key)
        {
            errors.push(error(format!(
                "attribute {} references unknown type {type_key}",
                attribute.key
            )));
        }
    }

    let mut type_keys = HashSet::new();
    for object_type in &defs.types {
        remember(object_type.id.as_uuid(), "type", errors);
        unique_key(&mut type_keys, &object_type.key, "type", errors);
        unique_refs(&object_type.attributes, &object_type.key, "type", errors);
        for attribute_id in &object_type.attributes {
            if !attribute_ids.contains(attribute_id) {
                errors.push(error(format!(
                    "type {} references unknown attribute {attribute_id}",
                    object_type.key
                )));
            }
        }
        match defs
            .attributes
            .iter()
            .find(|attribute| attribute.id == object_type.key_attribute)
        {
            None => errors.push(error(format!(
                "type {} references unknown key attribute {}",
                object_type.key, object_type.key_attribute
            ))),
            Some(attribute) => {
                let is_key = matches!(attribute.kind, AttributeKind::Text { .. })
                    && !attribute.multiple
                    && !attribute.language_dependent;
                if !is_key {
                    errors.push(error(format!(
                        "key attribute {} of type {} must be single-valued, language-independent text",
                        attribute.key, object_type.key
                    )));
                }
                if !object_type.attributes.contains(&attribute.id) {
                    errors.push(error(format!(
                        "key attribute {} is not an attribute of type {}",
                        attribute.key, object_type.key
                    )));
                }
            }
        }
    }

    let mut structure_keys = HashSet::new();
    for structure in &defs.structures {
        remember(structure.id.as_uuid(), "structure", errors);
        unique_key(&mut structure_keys, &structure.key, "structure", errors);
        if structure.groups.is_empty() {
            errors.push(error(format!("structure {} has no groups", structure.key)));
        }
        let mut group_keys = HashSet::new();
        let mut group_ids = HashSet::new();
        for group in &structure.groups {
            remember(group.id.as_uuid(), "group", errors);
            unique_key(
                &mut group_keys,
                &group.key,
                &format!("group in {}", structure.key),
                errors,
            );
            group_ids.insert(group.id);
        }
        for group in &structure.groups {
            if let Some(parent) = group.parent
                && !group_ids.contains(&parent)
            {
                errors.push(error(format!(
                    "group {} in structure {} has a parent outside the structure",
                    group.key, structure.key
                )));
            }
        }
        if has_cycle(&structure.groups) {
            errors.push(error(format!(
                "structure {} has a cycle in its groups",
                structure.key
            )));
        }
        for group in &structure.groups {
            unique_refs(&group.attributes, &group.key, "group", errors);
            for attribute_id in &group.attributes {
                if !attribute_ids.contains(attribute_id) {
                    errors.push(error(format!(
                        "group {} references unknown attribute {attribute_id}",
                        group.key
                    )));
                }
            }
        }
        let mut class_keys = HashSet::new();
        for class in &structure.classes {
            remember(class.id.as_uuid(), "class", errors);
            unique_key(
                &mut class_keys,
                &class.key,
                &format!("class in {}", structure.key),
                errors,
            );
            if !group_ids.contains(&class.group) {
                errors.push(error(format!(
                    "class {} in structure {} belongs to a group outside the structure",
                    class.key, structure.key
                )));
            }
            unique_refs(&class.attributes, &class.key, "class", errors);
            for attribute_id in &class.attributes {
                if !attribute_ids.contains(attribute_id) {
                    errors.push(error(format!(
                        "class {} references unknown attribute {attribute_id}",
                        class.key
                    )));
                }
            }
            if let Some(parent) = class.parent
                && !structure.classes.iter().any(|item| item.id == parent)
            {
                errors.push(error(format!(
                    "class {} in structure {} has a parent outside the structure",
                    class.key, structure.key
                )));
            }
        }
        if class_cycle(&structure.classes) {
            errors.push(error(format!(
                "structure {} has a cycle in its classes",
                structure.key
            )));
        }
    }
}

fn effective_attributes(
    start: ClassId,
    attrs: &HashMap<ClassId, Vec<AttributeId>>,
    parent: &HashMap<ClassId, Option<ClassId>>,
) -> HashSet<AttributeId> {
    let mut set = HashSet::new();
    let mut current = Some(start);
    let mut seen = HashSet::new();
    while let Some(id) = current {
        if !seen.insert(id) {
            break;
        }
        if let Some(list) = attrs.get(&id) {
            set.extend(list.iter().copied());
        }
        current = parent.get(&id).copied().flatten();
    }
    set
}

fn class_cycle(classes: &[braian_model::Class]) -> bool {
    let known: HashSet<ClassId> = classes.iter().map(|class| class.id).collect();
    let parent: HashMap<ClassId, ClassId> = classes
        .iter()
        .filter_map(|class| class.parent.map(|parent| (class.id, parent)))
        .collect();
    for class in classes {
        let mut seen = HashSet::new();
        let mut current = class.id;
        loop {
            if !seen.insert(current) {
                return true;
            }
            let Some(next) = parent.get(&current).copied() else {
                break;
            };
            if !known.contains(&next) {
                break;
            }
            current = next;
        }
    }
    false
}

fn unique_key(
    seen: &mut HashSet<String>,
    key: &str,
    kind: &str,
    errors: &mut Vec<DefinitionError>,
) {
    if key.is_empty() {
        errors.push(error(format!("empty {kind} key")));
    } else if braian_model::Uuid::parse_str(key).is_ok() {
        errors.push(error(format!(
            "{kind} key {key} must not be a UUID, so a value can be addressed by key or id"
        )));
    } else if !seen.insert(key.to_owned()) {
        errors.push(error(format!("duplicate {kind} key {key}")));
    }
}

fn unique_refs(ids: &[AttributeId], owner: &str, kind: &str, errors: &mut Vec<DefinitionError>) {
    let mut seen = HashSet::new();
    for id in ids {
        if !seen.insert(*id) {
            errors.push(error(format!(
                "{kind} {owner} lists attribute {id} more than once"
            )));
        }
    }
}

fn has_cycle(groups: &[braian_model::Group]) -> bool {
    let known: HashSet<GroupId> = groups.iter().map(|group| group.id).collect();
    let parent: HashMap<GroupId, GroupId> = groups
        .iter()
        .filter_map(|group| group.parent.map(|parent| (group.id, parent)))
        .collect();
    for group in groups {
        let mut seen = HashSet::new();
        let mut current = group.id;
        loop {
            if !seen.insert(current) {
                return true;
            }
            let Some(next) = parent.get(&current).copied() else {
                break;
            };
            if !known.contains(&next) {
                break;
            }
            current = next;
        }
    }
    false
}

fn error(message: String) -> DefinitionError {
    DefinitionError { message }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use braian_model::{
        Attribute, AttributeId, AttributeKind, Class, ClassId, DefinitionSet, ErrorCode, Group,
        GroupId, IndexFlags, LanguageTag, NumberValue, Object, ObjectId, ObjectType, Structure,
        StructureId, TypeId, Unit, UnitFamily, UnitFamilyId, Uuid, ValueOption,
    };
    use rust_decimal::Decimal;
    use serde_json::json;

    use super::{AttributeSource, CompiledModel};

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn tag(value: &str) -> LanguageTag {
        LanguageTag::parse(value).unwrap()
    }

    fn text_attribute(n: u128, key: &str) -> Attribute {
        Attribute {
            id: AttributeId::from_uuid(id(n)),
            key: key.to_owned(),
            labels: BTreeMap::new(),
            language_dependent: false,
            multiple: false,
            required: false,
            kind: AttributeKind::Text {
                max_length: Some(32),
                form: braian_model::TextForm::Short,
            },
            index: IndexFlags::default(),
        }
    }

    fn minimal() -> DefinitionSet {
        let sku = text_attribute(1, "sku");
        DefinitionSet {
            model_revision: 1,
            languages: vec![tag("sv")],
            unit_families: vec![],
            value_lists: vec![],
            attributes: vec![sku.clone()],
            types: vec![ObjectType {
                id: TypeId::from_uuid(id(2)),
                key: "item".to_owned(),
                labels: BTreeMap::new(),
                key_attribute: sku.id,
                attributes: vec![sku.id],
            }],
            structures: vec![],
        }
    }

    #[test]
    fn minimal_model_accepts_an_object_with_only_its_key() {
        let defs = minimal();
        let model = CompiledModel::compile(&defs).unwrap();
        let object = Object {
            id: ObjectId::from_uuid(id(9)),
            type_id: TypeId::from_uuid(id(2)),
            model_revision: 1,
            groups: BTreeMap::new(),
            values: BTreeMap::from([(AttributeId::from_uuid(id(1)), json!("SKU-1"))]),
        };
        assert!(model.validate(&object).is_empty());
    }

    #[test]
    fn rejects_a_key_that_parses_as_a_uuid() {
        let mut defs = minimal();
        defs.attributes[0].key = "018f0000-0000-7000-8000-000000000100".into();
        let errors = CompiledModel::compile(&defs).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("must not be a UUID"))
        );
    }

    #[test]
    fn a_patch_warns_about_errors_it_does_not_touch() {
        let mut defs = minimal();
        defs.languages.push(LanguageTag::parse("en-GB").unwrap());
        let name = text_attribute(8, "name");
        defs.attributes.push(Attribute {
            language_dependent: true,
            kind: AttributeKind::Text {
                max_length: Some(3),
                form: braian_model::TextForm::Long,
            },
            ..name.clone()
        });
        defs.types[0].attributes.push(name.id);
        let model = CompiledModel::compile(&defs).unwrap();
        let sku = AttributeId::from_uuid(id(1));
        let object = Object {
            id: ObjectId::from_uuid(id(9)),
            type_id: TypeId::from_uuid(id(2)),
            model_revision: 1,
            groups: BTreeMap::new(),
            values: BTreeMap::from([(sku, json!("SKU-1")), (name.id, json!({"sv": "1234"}))]),
        };
        let mut languages = BTreeMap::new();
        languages.insert(LanguageTag::parse("en-GB").unwrap(), Some(json!("ok")));
        let report = model.validate_patch(
            &object,
            &braian_model::ObjectPatch {
                values: braian_model::ValuePatch {
                    attributes: BTreeMap::from([(
                        name.id,
                        braian_model::AttributePatch::Languages { values: languages },
                    )]),
                },
                groups: BTreeMap::new(),
            },
        );
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(report.warnings.len(), 1);
        assert_eq!(report.warnings[0].code, ErrorCode::TextTooLong);
        assert_eq!(
            report.warnings[0]
                .language
                .as_ref()
                .map(LanguageTag::as_str),
            Some("sv")
        );
    }

    #[test]
    fn rejects_duplicate_attribute_keys() {
        let mut defs = minimal();
        defs.attributes.push(text_attribute(4, "sku"));
        let errors = CompiledModel::compile(&defs).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("duplicate attribute key"))
        );
    }

    #[test]
    fn rejects_a_localized_key_attribute() {
        let mut defs = minimal();
        defs.attributes[0].language_dependent = true;
        let errors = CompiledModel::compile(&defs).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("language-independent"))
        );
    }

    #[test]
    fn rejects_a_group_cycle() {
        let mut defs = minimal();
        let root = GroupId::from_uuid(id(10));
        let child = GroupId::from_uuid(id(11));
        defs.structures.push(Structure {
            id: StructureId::from_uuid(id(12)),
            key: "catalog".to_owned(),
            groups: vec![
                Group {
                    id: root,
                    key: "root".to_owned(),
                    parent: Some(child),
                    attributes: vec![],
                },
                Group {
                    id: child,
                    key: "child".to_owned(),
                    parent: Some(root),
                    attributes: vec![],
                },
            ],
            classes: vec![],
        });
        let errors = CompiledModel::compile(&defs).unwrap_err();
        assert!(errors.iter().any(|error| error.message.contains("cycle")));
    }

    #[test]
    fn rejects_a_unit_family_without_a_base_unit() {
        let mut defs = minimal();
        defs.unit_families.push(UnitFamily {
            id: UnitFamilyId::from_uuid(id(20)),
            key: "mass".to_owned(),
            dimension: Some("mass".to_owned()),
            base_unit: "kg".to_owned(),
            units: vec![Unit {
                code: "g".to_owned(),
                factor: Decimal::new(1, 3),
            }],
        });
        let errors = CompiledModel::compile(&defs).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("base unit"))
        );
    }

    #[test]
    fn rejects_duplicate_value_list_codes() {
        let mut defs = minimal();
        defs.attributes.push(Attribute {
            id: AttributeId::from_uuid(id(30)),
            key: "color".to_owned(),
            labels: BTreeMap::new(),
            language_dependent: false,
            multiple: false,
            required: false,
            kind: AttributeKind::ValueList {
                options: vec![
                    ValueOption {
                        code: "red".to_owned(),
                        labels: BTreeMap::new(),
                    },
                    ValueOption {
                        code: "red".to_owned(),
                        labels: BTreeMap::new(),
                    },
                ],
                list: None,
            },
            index: IndexFlags::default(),
        });
        let errors = CompiledModel::compile(&defs).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("duplicate value-list code"))
        );
    }

    #[test]
    fn rejects_a_dangling_attribute_reference() {
        let mut defs = minimal();
        defs.types[0]
            .attributes
            .push(AttributeId::from_uuid(id(99)));
        let errors = CompiledModel::compile(&defs).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("unknown attribute"))
        );
    }

    #[test]
    fn normalizes_grams_into_the_base_unit() {
        let mut defs = minimal();
        let family = UnitFamilyId::from_uuid(id(20));
        defs.unit_families.push(UnitFamily {
            id: family,
            key: "mass".to_owned(),
            dimension: Some("mass".to_owned()),
            base_unit: "kg".to_owned(),
            units: vec![
                Unit {
                    code: "kg".to_owned(),
                    factor: Decimal::ONE,
                },
                Unit {
                    code: "g".to_owned(),
                    factor: Decimal::new(1, 3),
                },
            ],
        });
        let weight = AttributeId::from_uuid(id(21));
        defs.attributes.push(Attribute {
            id: weight,
            key: "weight".to_owned(),
            labels: BTreeMap::new(),
            language_dependent: false,
            multiple: false,
            required: false,
            kind: AttributeKind::Number {
                unit_family: Some(family),
                min: Some(Decimal::ZERO),
                max: Some(Decimal::from(10)),
                integer: false,
            },
            index: IndexFlags::default(),
        });
        defs.types[0].attributes.push(weight);
        let model = CompiledModel::compile(&defs).unwrap();
        let normalized = model
            .normalize(
                weight,
                &NumberValue {
                    value: Decimal::from(5000),
                    unit: Some("g".into()),
                },
            )
            .unwrap();
        assert_eq!(normalized, Decimal::from(5));

        let object = Object {
            id: ObjectId::from_uuid(id(9)),
            type_id: TypeId::from_uuid(id(2)),
            model_revision: 1,
            groups: BTreeMap::new(),
            values: BTreeMap::from([
                (AttributeId::from_uuid(id(1)), json!("SKU-1")),
                (weight, json!({"value": "15000", "unit": "g"})),
            ]),
        };
        let errors = model.validate(&object);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, ErrorCode::AboveMax);
    }

    #[test]
    fn child_group_inherits_ancestor_classes_but_not_the_other_way() {
        let mut defs = minimal();
        let color = text_attribute(40, "color");
        let finish = text_attribute(41, "finish");
        defs.attributes.push(color.clone());
        defs.attributes.push(finish.clone());
        let root = GroupId::from_uuid(id(50));
        let child = GroupId::from_uuid(id(51));
        let structure = StructureId::from_uuid(id(52));
        defs.structures.push(Structure {
            id: structure,
            key: "catalog".to_owned(),
            groups: vec![
                Group {
                    id: root,
                    key: "root".to_owned(),
                    parent: None,
                    attributes: vec![],
                },
                Group {
                    id: child,
                    key: "child".to_owned(),
                    parent: Some(root),
                    attributes: vec![],
                },
            ],
            classes: vec![
                Class {
                    id: ClassId::from_uuid(id(53)),
                    key: "root".to_owned(),
                    group: root,
                    parent: None,
                    attributes: vec![color.id],
                },
                Class {
                    id: ClassId::from_uuid(id(54)),
                    key: "child".to_owned(),
                    group: child,
                    parent: None,
                    attributes: vec![finish.id],
                },
            ],
        });
        let model = CompiledModel::compile(&defs).unwrap();
        let sku = AttributeId::from_uuid(id(1));
        let type_id = TypeId::from_uuid(id(2));

        let on_child = object(
            type_id,
            structure,
            child,
            vec![
                (sku, json!("SKU-1")),
                (color.id, json!("red")),
                (finish.id, json!("matte")),
            ],
        );
        assert!(model.validate(&on_child).is_empty());

        let finish_on_root = object(
            type_id,
            structure,
            root,
            vec![(sku, json!("SKU-1")), (finish.id, json!("matte"))],
        );
        let errors = model.validate(&finish_on_root);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, ErrorCode::AttributeNotApplicable);
        assert_eq!(errors[0].attribute, Some(finish.id));

        let color_on_root = object(
            type_id,
            structure,
            root,
            vec![(sku, json!("SKU-1")), (color.id, json!("red"))],
        );
        assert!(model.validate(&color_on_root).is_empty());
    }

    #[test]
    fn integer_range_and_object_ref_have_their_own_codes() {
        let mut defs = minimal();
        let family = UnitFamilyId::from_uuid(id(20));
        defs.unit_families.push(UnitFamily {
            id: family,
            key: "length".to_owned(),
            dimension: Some("length".to_owned()),
            base_unit: "mm".to_owned(),
            units: vec![Unit {
                code: "mm".to_owned(),
                factor: Decimal::ONE,
            }],
        });
        let count = AttributeId::from_uuid(id(21));
        let span = AttributeId::from_uuid(id(22));
        let link = AttributeId::from_uuid(id(23));
        defs.attributes.push(Attribute {
            id: count,
            key: "count".to_owned(),
            labels: BTreeMap::new(),
            language_dependent: false,
            multiple: false,
            required: false,
            kind: AttributeKind::Number {
                unit_family: None,
                min: None,
                max: None,
                integer: true,
            },
            index: IndexFlags::default(),
        });
        defs.attributes.push(Attribute {
            id: span,
            key: "span".to_owned(),
            labels: BTreeMap::new(),
            language_dependent: false,
            multiple: false,
            required: false,
            kind: AttributeKind::Range {
                unit_family: Some(family),
            },
            index: IndexFlags::default(),
        });
        defs.attributes.push(Attribute {
            id: link,
            key: "replaces".to_owned(),
            labels: BTreeMap::new(),
            language_dependent: false,
            multiple: false,
            required: false,
            kind: AttributeKind::ObjectRef {
                type_key: Some("item".to_owned()),
            },
            index: IndexFlags::default(),
        });
        defs.types[0].attributes.extend([count, span, link]);
        let model = CompiledModel::compile(&defs).unwrap();
        let sku = AttributeId::from_uuid(id(1));
        let mut values = BTreeMap::new();
        values.insert(sku, json!("SKU-1"));
        values.insert(count, json!({"value": "1.5"}));
        values.insert(span, json!({"min": "9", "max": "2", "unit": "mm"}));
        values.insert(link, json!("not-an-id"));
        let object = Object {
            id: ObjectId::from_uuid(id(9)),
            type_id: TypeId::from_uuid(id(2)),
            model_revision: 1,
            groups: BTreeMap::new(),
            values,
        };
        let codes: Vec<_> = model
            .validate(&object)
            .into_iter()
            .map(|error| error.code)
            .collect();
        assert!(codes.contains(&ErrorCode::NotInteger));
        assert!(codes.contains(&ErrorCode::RangeInverted));
        assert!(codes.contains(&ErrorCode::InvalidObjectRef));
    }

    #[test]
    fn a_child_class_inherits_attributes_from_its_parent_class() {
        let mut defs = minimal();
        let color = text_attribute(40, "color");
        let finish = text_attribute(41, "finish");
        defs.attributes.push(color.clone());
        defs.attributes.push(finish.clone());
        let root = GroupId::from_uuid(id(50));
        let child = GroupId::from_uuid(id(51));
        let other = GroupId::from_uuid(id(55));
        let structure = StructureId::from_uuid(id(52));
        let base = ClassId::from_uuid(id(53));
        defs.structures.push(Structure {
            id: structure,
            key: "tools".into(),
            groups: vec![
                Group {
                    id: root,
                    key: "root".into(),
                    parent: None,
                    attributes: vec![],
                },
                Group {
                    id: child,
                    key: "child".into(),
                    parent: Some(root),
                    attributes: vec![],
                },
                Group {
                    id: other,
                    key: "other".into(),
                    parent: None,
                    attributes: vec![],
                },
            ],
            classes: vec![
                Class {
                    id: base,
                    key: "base".into(),
                    group: other,
                    parent: None,
                    attributes: vec![color.id],
                },
                Class {
                    id: ClassId::from_uuid(id(54)),
                    key: "drill".into(),
                    group: child,
                    parent: Some(base),
                    attributes: vec![finish.id],
                },
            ],
        });
        let model = CompiledModel::compile(&defs).unwrap();
        let sku = AttributeId::from_uuid(id(1));
        let type_id = TypeId::from_uuid(id(2));
        let on_child = object(
            type_id,
            structure,
            child,
            vec![
                (sku, json!("SKU-1")),
                (color.id, json!("red")),
                (finish.id, json!("matte")),
            ],
        );
        assert!(model.validate(&on_child).is_empty());
        let color_on_root = object(
            type_id,
            structure,
            root,
            vec![(sku, json!("SKU-1")), (color.id, json!("red"))],
        );
        assert_eq!(
            model.validate(&color_on_root)[0].code,
            ErrorCode::AttributeNotApplicable
        );
    }

    #[test]
    fn effective_attributes_follow_the_type_every_structure_and_its_ancestors() {
        let name = text_attribute(1, "name");
        let weight = text_attribute(2, "weight");
        let length = text_attribute(3, "length");
        let campaign_only = text_attribute(4, "badge");
        let product = TypeId::from_uuid(id(10));
        let assortment = StructureId::from_uuid(id(20));
        let outdoor = GroupId::from_uuid(id(21));
        let tents = GroupId::from_uuid(id(22));
        let campaigns = StructureId::from_uuid(id(30));
        let summer = GroupId::from_uuid(id(31));
        let defs = DefinitionSet {
            model_revision: 1,
            languages: vec![tag("en")],
            unit_families: vec![],
            value_lists: vec![],
            attributes: vec![
                name.clone(),
                weight.clone(),
                length.clone(),
                campaign_only.clone(),
            ],
            types: vec![ObjectType {
                id: product,
                key: "product".into(),
                labels: BTreeMap::new(),
                key_attribute: name.id,
                attributes: vec![name.id, weight.id],
            }],
            structures: vec![
                Structure {
                    id: assortment,
                    key: "assortment".into(),
                    groups: vec![
                        Group {
                            id: outdoor,
                            key: "outdoor".into(),
                            parent: None,
                            attributes: vec![weight.id],
                        },
                        Group {
                            id: tents,
                            key: "tents".into(),
                            parent: Some(outdoor),
                            attributes: vec![length.id],
                        },
                    ],
                    classes: vec![],
                },
                Structure {
                    id: campaigns,
                    key: "campaigns".into(),
                    groups: vec![Group {
                        id: summer,
                        key: "summer".into(),
                        parent: None,
                        attributes: vec![weight.id, campaign_only.id],
                    }],
                    classes: vec![],
                },
            ],
        };
        let model = CompiledModel::compile(&defs).unwrap();
        let effective = model
            .effective_attributes(
                product,
                &BTreeMap::from([(assortment, tents), (campaigns, summer)]),
            )
            .expect("known type and groups");
        let by_id = |needle: AttributeId| {
            effective
                .iter()
                .find(|item| item.id == needle)
                .unwrap_or_else(|| panic!("missing {needle}"))
        };
        assert_eq!(effective.len(), 4, "type + both structures, deduped");
        assert_eq!(
            by_id(weight.id).sources,
            vec![
                AttributeSource::Type {
                    key: "product".into()
                },
                AttributeSource::Group {
                    structure: "assortment".into(),
                    group: "outdoor".into(),
                },
                AttributeSource::Group {
                    structure: "campaigns".into(),
                    group: "summer".into(),
                },
            ]
        );
        assert_eq!(
            by_id(length.id).sources,
            vec![AttributeSource::Group {
                structure: "assortment".into(),
                group: "tents".into(),
            }]
        );
        assert!(!by_id(length.id).sources.iter().any(
            |source| matches!(source, AttributeSource::Group { group, .. } if group == "outdoor")
        ));
        let ids = model
            .applicable_ids(
                product,
                &BTreeMap::from([(assortment, tents), (campaigns, summer)]),
            )
            .expect("applicable");
        let mut from_effective: Vec<_> = effective.iter().map(|item| item.id).collect();
        from_effective.sort();
        assert_eq!(ids, from_effective);
    }

    #[test]
    fn a_class_attribute_still_applies_to_groups_below_its_node() {
        let name = text_attribute(1, "name");
        let weight = text_attribute(2, "weight");
        let product = TypeId::from_uuid(id(10));
        let assortment = StructureId::from_uuid(id(20));
        let outdoor = GroupId::from_uuid(id(21));
        let tents = GroupId::from_uuid(id(22));
        let defs = DefinitionSet {
            model_revision: 1,
            languages: vec![tag("en")],
            unit_families: vec![],
            value_lists: vec![],
            attributes: vec![name.clone(), weight.clone()],
            types: vec![ObjectType {
                id: product,
                key: "product".into(),
                labels: BTreeMap::new(),
                key_attribute: name.id,
                attributes: vec![name.id],
            }],
            structures: vec![Structure {
                id: assortment,
                key: "assortment".into(),
                groups: vec![
                    Group {
                        id: outdoor,
                        key: "outdoor".into(),
                        parent: None,
                        attributes: vec![],
                    },
                    Group {
                        id: tents,
                        key: "tents".into(),
                        parent: Some(outdoor),
                        attributes: vec![],
                    },
                ],
                classes: vec![Class {
                    id: ClassId::from_uuid(id(40)),
                    key: "outdoor".into(),
                    group: outdoor,
                    parent: None,
                    attributes: vec![weight.id],
                }],
            }],
        };
        let model = CompiledModel::compile(&defs).unwrap();
        let effective = model
            .effective_attributes(product, &BTreeMap::from([(assortment, tents)]))
            .expect("effective");
        let weight_row = effective
            .iter()
            .find(|item| item.id == weight.id)
            .expect("weight");
        assert_eq!(
            weight_row.sources,
            vec![AttributeSource::Group {
                structure: "assortment".into(),
                group: "outdoor".into(),
            }]
        );
    }

    fn object(
        type_id: TypeId,
        structure: StructureId,
        group: GroupId,
        values: Vec<(AttributeId, serde_json::Value)>,
    ) -> Object {
        Object {
            id: ObjectId::from_uuid(id(90)),
            type_id,
            model_revision: 1,
            groups: BTreeMap::from([(structure, group)]),
            values: values.into_iter().collect(),
        }
    }
}
