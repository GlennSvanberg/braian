//! Which objects a link type may connect, and how many times.

use std::collections::{BTreeMap, BTreeSet};

use crate::definition::{
    Attribute, AttributeKind, Cardinality, DefinitionSet, LinkLabel, LinkSpec, ObjectType, TypeKind,
};
use crate::id::{AttributeId, TypeId};
use crate::language::LanguageTag;
use crate::refs;
use crate::standard::{attach_standard_attributes, ensure_standard_rows, standard_ids};

/// Why a link between two objects is not allowed.
pub fn explain_link(
    defs: &DefinitionSet,
    link_type: &str,
    from_type: &str,
    to_type: &str,
    from_existing: i64,
    to_existing: i64,
) -> Result<(), String> {
    let link =
        find_type(defs, link_type).ok_or_else(|| format!("unknown link type {link_type}"))?;
    if !link.is_link() {
        return Err(format!("{link_type} is not a link type"));
    }
    let spec = link
        .link
        .as_ref()
        .ok_or_else(|| format!("link type {} has no ends", link.key))?;
    let from = find_type(defs, from_type).ok_or_else(|| format!("unknown type {from_type}"))?;
    let to = find_type(defs, to_type).ok_or_else(|| format!("unknown type {to_type}"))?;
    if from.kind != TypeKind::Entity {
        return Err(format!("{} cannot be the source of a link", from.key));
    }
    if to.kind != TypeKind::Entity {
        return Err(format!("{} cannot be the target of a link", to.key));
    }
    if !spec.from_types.contains(&from.id) {
        return Err(format!(
            "link type {} does not allow {} as the source",
            link.key, from.key
        ));
    }
    if !spec.to_types.contains(&to.id) {
        return Err(format!(
            "link type {} does not allow {} as the target",
            link.key, to.key
        ));
    }
    if !spec.from_cardinality.allows(from_existing) {
        return Err(format!(
            "link type {} allows one link from each source",
            link.key
        ));
    }
    if !spec.to_cardinality.allows(to_existing) {
        return Err(format!(
            "link type {} allows one link to each target",
            link.key
        ));
    }
    Ok(())
}

pub fn find_type<'a>(
    defs: &'a DefinitionSet,
    token: &str,
) -> Option<&'a crate::definition::ObjectType> {
    let token = token.trim();
    if let Ok(id) = refs::type_id(defs, token) {
        return defs.types.iter().find(|item| item.id == id);
    }
    defs.types
        .iter()
        .find(|item| item.key.eq_ignore_ascii_case(token))
}

/// Types named on a link end must already exist and must be entities.
pub fn check_ends(defs: &DefinitionSet, types: &[TypeId], side: &str) -> Result<(), String> {
    if types.is_empty() {
        return Err(format!("{side} needs at least one type"));
    }
    for id in types {
        let Some(object_type) = defs.types.iter().find(|item| item.id == *id) else {
            return Err(format!("unknown type {id} on {side}"));
        };
        if object_type.kind != TypeKind::Entity {
            return Err(format!(
                "{side} type {} has to be an entity type",
                object_type.key
            ));
        }
    }
    Ok(())
}

pub fn parse_cardinality(text: &str) -> Result<Cardinality, String> {
    Cardinality::parse(text).ok_or_else(|| format!("cardinality {text} must be one or many"))
}

/// One object-reference attribute that becomes a link type.
///
/// The referenced object is the source (`from`). The object that held the
/// attribute is the target (`to`), so the old attribute label stays on the
/// holder and the inverse label stays on the referenced object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefLinkPlan {
    pub attribute_id: AttributeId,
    pub link_type_id: TypeId,
    /// `one` when each holder stored at most one reference.
    pub to_one: bool,
}

/// Latest definition set after object references have become link types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefConversion {
    pub defs: DefinitionSet,
    pub plans: Vec<RefLinkPlan>,
    /// Attributes whose stored values are removed, including references that could not become a link.
    pub strip: Vec<AttributeId>,
}

/// Turns each `object_ref` attribute into a link type and drops it from the latest set.
///
/// `max_values` is the most references one object stored for that attribute.
/// More than one widens the holder's side to `many`, so a stored value is not rejected.
/// An attribute used as a type's key stays; it cannot become a link.
/// Returns nothing when there is no convertible attribute.
pub fn convert_object_refs(
    defs: &DefinitionSet,
    max_values: &BTreeMap<AttributeId, i64>,
) -> Option<RefConversion> {
    let key_attributes: BTreeSet<AttributeId> = defs
        .types
        .iter()
        .map(|object_type| object_type.key_attribute)
        .collect();
    let convertible: Vec<Attribute> = defs
        .attributes
        .iter()
        .filter(|attribute| {
            matches!(attribute.kind, AttributeKind::ObjectRef { .. })
                && !key_attributes.contains(&attribute.id)
        })
        .cloned()
        .collect();
    if convertible.is_empty() {
        return None;
    }
    let mut next = defs.clone();
    ensure_standard_rows(&mut next);
    let mut plans = Vec::with_capacity(convertible.len());
    let mut strip = Vec::with_capacity(convertible.len());
    for attribute in &convertible {
        let holders: Vec<ObjectType> = next
            .types
            .iter()
            .filter(|object_type| {
                object_type.kind.is_entity() && object_type.attributes.contains(&attribute.id)
            })
            .cloned()
            .collect();
        let AttributeKind::ObjectRef { type_key } = &attribute.kind else {
            continue;
        };
        let referenced = type_key
            .as_ref()
            .and_then(|token| find_type(&next, token))
            .filter(|object_type| object_type.kind.is_entity())
            .cloned();
        let from_types: Vec<TypeId> = match &referenced {
            Some(object_type) => vec![object_type.id],
            None => next
                .types
                .iter()
                .filter(|object_type| object_type.kind.is_entity())
                .map(|object_type| object_type.id)
                .collect(),
        };
        let to_types: Vec<TypeId> = holders.iter().map(|object_type| object_type.id).collect();
        strip.push(attribute.id);
        if from_types.is_empty() || to_types.is_empty() {
            detach_attribute(&mut next, attribute.id);
            continue;
        }
        let same_type = to_types.len() == 1 && from_types == to_types;
        let preferred = if same_type {
            attribute.key.clone()
        } else if let (Some(referenced), [holder]) = (&referenced, holders.as_slice()) {
            format!("{}_{}", referenced.key, plural_key(&holder.key))
        } else {
            format!("{}_link", attribute.key)
        };
        let existing = next
            .types
            .iter()
            .find(|object_type| {
                object_type.is_link() && object_type.key.eq_ignore_ascii_case(&preferred)
            })
            .map(|object_type| object_type.id);
        let type_key = if existing.is_some() {
            preferred
        } else {
            unique_type_key(&next, &preferred)
        };
        let link_type_id = existing.unwrap_or_else(|| stable_type_id(&attribute.id));
        let widest = max_values.get(&attribute.id).copied().unwrap_or(0);
        let to_one = !attribute.multiple && widest <= 1;
        if existing.is_none() {
            let holder_refs: Vec<&ObjectType> = holders.iter().collect();
            let (from_label, to_label) = side_labels(&next, attribute, &holder_refs, same_type);
            let mut link_type = ObjectType {
                id: link_type_id,
                key: type_key,
                labels: attribute_labels(&next, attribute),
                singular: BTreeMap::new(),
                plural: BTreeMap::new(),
                key_attribute: standard_ids().created_at,
                attributes: vec![],
                structures: Some(vec![]),
                defaults: BTreeMap::new(),
                kind: TypeKind::Link,
                link: Some(LinkSpec {
                    from_types,
                    to_types,
                    from_label,
                    to_label,
                    from_cardinality: Cardinality::Many,
                    to_cardinality: if to_one {
                        Cardinality::One
                    } else {
                        Cardinality::Many
                    },
                }),
            };
            attach_standard_attributes(&mut link_type);
            next.types.push(link_type);
        }
        detach_attribute(&mut next, attribute.id);
        plans.push(RefLinkPlan {
            attribute_id: attribute.id,
            link_type_id,
            to_one,
        });
    }
    Some(RefConversion {
        defs: next,
        plans,
        strip,
    })
}

fn detach_attribute(defs: &mut DefinitionSet, id: AttributeId) {
    defs.attributes.retain(|attribute| attribute.id != id);
    for object_type in &mut defs.types {
        object_type.attributes.retain(|attribute| *attribute != id);
    }
    for structure in &mut defs.structures {
        for group in &mut structure.groups {
            group.attributes.retain(|attribute| *attribute != id);
        }
        for class in &mut structure.classes {
            class.attributes.retain(|attribute| *attribute != id);
        }
    }
}

fn side_labels(
    defs: &DefinitionSet,
    attribute: &Attribute,
    holders: &[&ObjectType],
    same_type: bool,
) -> (LinkLabel, LinkLabel) {
    let from_text = attribute
        .labels
        .values()
        .next()
        .cloned()
        .unwrap_or_else(|| attribute.key.clone());
    let from_label = LinkLabel {
        key: unique_side_key(&attribute.key, None),
        labels: filled_labels(defs, &from_text),
    };
    let (to_key, to_text) = if same_type {
        let key = if attribute.key == "replaces" {
            "replaced_by".to_owned()
        } else {
            format!("{}_by", attribute.key)
        };
        (key, inverse_text(&from_text))
    } else if holders.len() == 1 {
        let holder = holders[0];
        (plural_key(&holder.key), holder.plural_label("en"))
    } else {
        (
            plural_key(&attribute.key),
            crate::definition::default_plural(&from_text),
        )
    };
    let to_label = LinkLabel {
        key: unique_side_key(&to_key, Some(&from_label.key)),
        labels: filled_labels(defs, &to_text),
    };
    (from_label, to_label)
}

fn inverse_text(label: &str) -> String {
    if label.eq_ignore_ascii_case("replaces") {
        "Replaced by".to_owned()
    } else {
        format!("{label} by")
    }
}

fn unique_side_key(base: &str, taken: Option<&str>) -> String {
    let mut key = slug_key(base);
    if taken.is_some_and(|other| other.eq_ignore_ascii_case(&key)) {
        key = format!("{key}_to");
    }
    key
}

fn plural_key(key: &str) -> String {
    slug_key(&crate::definition::default_plural(key))
}

fn slug_key(text: &str) -> String {
    let key: String = text
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    let key = key.trim_matches('_').to_owned();
    if key.is_empty() {
        "link".to_owned()
    } else {
        key
    }
}

fn unique_type_key(defs: &DefinitionSet, base: &str) -> String {
    let base = slug_key(base);
    if !defs
        .types
        .iter()
        .any(|object_type| object_type.key.eq_ignore_ascii_case(&base))
    {
        return base;
    }
    let mut n = 2u32;
    loop {
        let key = format!("{base}_{n}");
        if !defs
            .types
            .iter()
            .any(|object_type| object_type.key.eq_ignore_ascii_case(&key))
        {
            return key;
        }
        n += 1;
    }
}

fn attribute_labels(defs: &DefinitionSet, attribute: &Attribute) -> BTreeMap<LanguageTag, String> {
    if !attribute.labels.is_empty() {
        return attribute.labels.clone();
    }
    filled_labels(defs, &attribute.key)
}

fn filled_labels(defs: &DefinitionSet, text: &str) -> BTreeMap<LanguageTag, String> {
    let mut labels = BTreeMap::new();
    if let Some(tag) = defs
        .languages
        .iter()
        .find(|tag| tag.as_str() == "en")
        .cloned()
        .or_else(|| defs.languages.first().cloned())
    {
        labels.insert(tag, text.to_owned());
    }
    labels
}

fn stable_type_id(attribute: &AttributeId) -> TypeId {
    let label = format!("object-ref:{}", attribute.as_uuid());
    TypeId::from_uuid(stable_uuid(&label))
}

fn stable_uuid(label: &str) -> uuid::Uuid {
    let mut hash = 0xcbf29ce484222325u128;
    for byte in label.as_bytes() {
        hash ^= u128::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let mut bytes = hash.to_le_bytes();
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::definition::{LinkLabel, LinkSpec, ObjectType, TypeKind};
    use crate::id::AttributeId;
    use crate::id::TypeId;
    use crate::language::LanguageTag;
    use uuid::Uuid;

    fn label(key: &str, text: &str) -> LinkLabel {
        let mut labels = BTreeMap::new();
        labels.insert(LanguageTag::parse("en").unwrap(), text.to_owned());
        LinkLabel {
            key: key.to_owned(),
            labels,
        }
    }

    fn entity(n: u128, key: &str) -> ObjectType {
        ObjectType {
            id: TypeId::from_uuid(Uuid::from_u128(n)),
            key: key.to_owned(),
            labels: BTreeMap::new(),
            singular: BTreeMap::new(),
            plural: BTreeMap::new(),
            key_attribute: AttributeId::from_uuid(Uuid::from_u128(9)),
            attributes: vec![],
            structures: None,
            defaults: BTreeMap::new(),
            kind: TypeKind::Entity,
            link: None,
        }
    }

    fn defs() -> DefinitionSet {
        let article = entity(1, "article");
        let product = entity(2, "product");
        let mut pairs = entity(3, "pairs_with");
        pairs.kind = TypeKind::Link;
        pairs.link = Some(LinkSpec {
            from_types: vec![article.id],
            to_types: vec![article.id, product.id],
            from_label: label("paired_with", "Pairs with"),
            to_label: label("pairs_with", "Pairs with"),
            from_cardinality: Cardinality::Many,
            to_cardinality: Cardinality::One,
        });
        DefinitionSet {
            model_revision: 1,
            languages: vec![LanguageTag::parse("en").unwrap()],
            unit_families: vec![],
            value_lists: vec![],
            attributes: vec![],
            types: vec![article, product, pairs],
            structures: vec![],
        }
    }

    fn attribute(
        id: u128,
        key: &str,
        type_key: &str,
        multiple: bool,
    ) -> crate::definition::Attribute {
        use crate::definition::{Attribute, AttributeKind, IndexFlags};
        Attribute {
            id: AttributeId::from_uuid(Uuid::from_u128(id)),
            key: key.to_owned(),
            labels: BTreeMap::from([(LanguageTag::parse("en").unwrap(), key.to_owned())]),
            language_dependent: false,
            multiple,
            required: true,
            kind: AttributeKind::ObjectRef {
                type_key: Some(type_key.to_owned()),
            },
            index: IndexFlags::default(),
        }
    }

    #[test]
    fn an_object_reference_becomes_a_link_with_the_holder_as_target() {
        let mut defs = defs();
        let product_attr = attribute(10, "product", "product", false);
        let replaces = attribute(11, "replaces", "article", false);
        defs.types[0].attributes = vec![product_attr.id, replaces.id];
        defs.attributes = vec![product_attr.clone(), replaces.clone()];
        let mut widest = BTreeMap::new();
        widest.insert(product_attr.id, 2);
        let converted = convert_object_refs(&defs, &widest).unwrap();
        assert!(converted.defs.attributes.iter().all(|item| !matches!(
            item.kind,
            crate::definition::AttributeKind::ObjectRef { .. }
        )));
        let product_articles = converted
            .defs
            .types
            .iter()
            .find(|item| item.key == "product_articles")
            .unwrap();
        let spec = product_articles.link.as_ref().unwrap();
        assert_eq!(spec.from_types, vec![TypeId::from_uuid(Uuid::from_u128(2))]);
        assert_eq!(spec.to_types, vec![TypeId::from_uuid(Uuid::from_u128(1))]);
        assert_eq!(spec.from_label.key, "product");
        assert_eq!(spec.to_label.key, "articles");
        assert_eq!(spec.to_cardinality, Cardinality::Many);
        let replaces_type = converted
            .defs
            .types
            .iter()
            .find(|item| item.key == "replaces")
            .unwrap();
        let spec = replaces_type.link.as_ref().unwrap();
        assert_eq!(spec.from_label.key, "replaces");
        assert_eq!(spec.to_label.key, "replaced_by");
        assert_eq!(spec.to_cardinality, Cardinality::One);
        assert!(convert_object_refs(&converted.defs, &widest).is_none());
    }

    #[test]
    fn cardinality_and_allowed_types_are_checked_per_direction() {
        let defs = defs();
        assert!(explain_link(&defs, "pairs_with", "article", "article", 4, 0).is_ok());
        let wrong = explain_link(&defs, "pairs_with", "product", "article", 0, 0).unwrap_err();
        assert!(wrong.contains("source"), "{wrong}");
        let capped = explain_link(&defs, "pairs_with", "article", "product", 0, 1).unwrap_err();
        assert!(capped.contains("one link to each target"), "{capped}");
        assert!(explain_link(&defs, "pairs_with", "article", "product", 0, 0).is_ok());
        let missing = check_ends(&defs, &[], "from").unwrap_err();
        assert!(missing.contains("at least one"));
    }
}
