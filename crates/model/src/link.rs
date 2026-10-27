//! Which objects a link type may connect, and how many times.

use crate::definition::{Cardinality, DefinitionSet, TypeKind};
use crate::id::TypeId;
use crate::refs;

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
