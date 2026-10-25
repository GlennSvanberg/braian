use std::collections::BTreeMap;

use braian_model::{
    Attribute, AttributeId, AttributeKind, Class, ClassId, DefinitionSet, Group, GroupId,
    IndexFlags, LanguageTag, ObjectType, Structure, StructureId, TextForm, TypeId, Unit,
    UnitFamily, UnitFamilyId,
};
use rust_decimal::Decimal;

use crate::Tier;
use crate::ids::Ids;

const LANGUAGE_TAGS: [&str; 20] = [
    "sv", "en-GB", "de", "fr", "es", "sv-SE", "it", "nl", "pl", "fi", "da", "nb", "pt", "en-US",
    "cs", "fr-CA", "hu", "ro", "de-AT", "pt-BR",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Json,
    Yaml,
}

impl ExportFormat {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "json" => Some(Self::Json),
            "yaml" => Some(Self::Yaml),
            _ => None,
        }
    }
}

/// Caps on how many attributes carry each index flag.
///
/// `None` keeps the tier's built-in pattern (every other text or list attribute
/// searchable, every third filterable, odd ordered ones sortable). A cap keeps the
/// first `n` attributes, in key order, that the pattern would flag. Object bytes do
/// not depend on the budget.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IndexBudget {
    pub searchable: Option<usize>,
    pub filterable: Option<usize>,
    pub sortable: Option<usize>,
}

pub fn definitions(tier: Tier, seed: u64) -> DefinitionSet {
    definitions_with(tier, seed, IndexBudget::default())
}

/// [`definitions`] with the index flags capped by `budget`.
pub fn definitions_with(tier: Tier, seed: u64, budget: IndexBudget) -> DefinitionSet {
    let mut ids = Ids::new(seed);
    let languages: Vec<LanguageTag> = LANGUAGE_TAGS
        .iter()
        .take(tier.language_count())
        .map(|tag| LanguageTag::parse(tag).expect("static language tag"))
        .collect();
    let label_language = languages[0].clone();

    let mass = unit_family(
        &mut ids,
        "mass",
        "kg",
        &[
            ("kg", Decimal::ONE),
            ("g", Decimal::new(1, 3)),
            ("mg", Decimal::new(1, 6)),
        ],
    );
    let length = unit_family(
        &mut ids,
        "length",
        "m",
        &[
            ("m", Decimal::ONE),
            ("cm", Decimal::new(1, 2)),
            ("mm", Decimal::new(1, 3)),
        ],
    );

    let count = tier.attribute_count();
    let localized_until = count * 30 / 100;
    let mut attributes = Vec::with_capacity(count + 1);
    let mut buckets: [Vec<AttributeId>; 5] =
        [Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    let mut used = IndexBudget {
        searchable: Some(0),
        filterable: Some(0),
        sortable: Some(0),
    };
    for index in 0..count {
        let id = AttributeId::from_uuid(ids.next());
        let language_dependent = (1..=localized_until).contains(&index);
        let multiple = index != 0 && index % 10 == 9;
        let kind = kind_for(index, mass.id, length.id);
        let key = if index == 0 {
            "sku".to_owned()
        } else {
            format!("a{index:04}")
        };
        attributes.push(Attribute {
            id,
            key: key.clone(),
            labels: label(&label_language, format!("Attribute {key}")),
            language_dependent,
            multiple,
            required: false,
            index: capped(flags(index, &kind, multiple), budget, &mut used),
            kind,
        });
        buckets[index % 5].push(id);
    }

    let unused_id = AttributeId::from_uuid(ids.next());
    attributes.push(Attribute {
        id: unused_id,
        key: "unused".to_owned(),
        labels: label(&label_language, "Unused".to_owned()),
        language_dependent: false,
        multiple: false,
        required: false,
        kind: AttributeKind::Text {
            max_length: Some(32),
            form: TextForm::Short,
        },
        index: IndexFlags::default(),
    });

    let item = TypeId::from_uuid(ids.next());
    let catalog = StructureId::from_uuid(ids.next());
    let channel = StructureId::from_uuid(ids.next());
    let root = GroupId::from_uuid(ids.next());
    let mid = GroupId::from_uuid(ids.next());
    let leaf = GroupId::from_uuid(ids.next());
    let flat = GroupId::from_uuid(ids.next());

    DefinitionSet {
        model_revision: 1,
        languages,
        unit_families: vec![mass, length],
        value_lists: vec![],
        attributes,
        types: vec![ObjectType {
            id: item,
            key: "item".to_owned(),
            labels: label(&label_language, "Item".to_owned()),
            key_attribute: buckets[0][0],
            attributes: buckets[0].clone(),
        }],
        structures: vec![
            Structure {
                id: catalog,
                key: "catalog".to_owned(),
                groups: vec![
                    Group {
                        id: root,
                        key: "root".to_owned(),
                        parent: None,
                        attributes: vec![],
                    },
                    Group {
                        id: mid,
                        key: "mid".to_owned(),
                        parent: Some(root),
                        attributes: vec![],
                    },
                    Group {
                        id: leaf,
                        key: "leaf".to_owned(),
                        parent: Some(mid),
                        attributes: vec![],
                    },
                ],
                classes: vec![
                    class(&mut ids, "root", root, &buckets[1]),
                    class(&mut ids, "mid", mid, &buckets[2]),
                    class(&mut ids, "leaf", leaf, &buckets[3]),
                ],
            },
            Structure {
                id: channel,
                key: "channel".to_owned(),
                groups: vec![Group {
                    id: flat,
                    key: "flat".to_owned(),
                    parent: None,
                    attributes: vec![],
                }],
                classes: vec![class(&mut ids, "flat", flat, &buckets[4])],
            },
        ],
    }
}

pub fn export(defs: &DefinitionSet, format: ExportFormat) -> Result<String, serde_norway::Error> {
    match format {
        ExportFormat::Json => {
            Ok(serde_json::to_string_pretty(defs).expect("definitions serialize"))
        }
        ExportFormat::Yaml => serde_norway::to_string(defs),
    }
}

fn unit_family(ids: &mut Ids, key: &str, base: &str, units: &[(&str, Decimal)]) -> UnitFamily {
    UnitFamily {
        id: UnitFamilyId::from_uuid(ids.next()),
        key: key.to_owned(),
        dimension: Some(key.to_owned()),
        base_unit: base.to_owned(),
        units: units
            .iter()
            .map(|(code, factor)| Unit {
                code: (*code).to_owned(),
                factor: *factor,
            })
            .collect(),
    }
}

fn kind_for(index: usize, mass: UnitFamilyId, length: UnitFamilyId) -> AttributeKind {
    match index % 6 {
        0 => AttributeKind::Text {
            max_length: Some(if index == 0 { 64 } else { 200 }),
            form: if index == 0 {
                TextForm::Short
            } else {
                TextForm::Long
            },
        },
        1 => {
            let generation = index / 6;
            let family = if generation % 4 == 2 { length } else { mass };
            AttributeKind::Number {
                unit_family: generation.is_multiple_of(2).then_some(family),
                min: Some(Decimal::ZERO),
                max: Some(Decimal::from(1000)),
                integer: false,
            }
        }
        2 => AttributeKind::ValueList {
            options: (0..5)
                .map(|option| braian_model::ValueOption {
                    code: format!("v{option}"),
                    labels: BTreeMap::new(),
                })
                .collect(),
            list: None,
        },
        3 => AttributeKind::Boolean,
        4 => AttributeKind::Date,
        _ => AttributeKind::FileRef {
            media: index.is_multiple_of(12),
        },
    }
}

fn flags(index: usize, kind: &AttributeKind, multiple: bool) -> IndexFlags {
    let text_or_list = matches!(
        kind,
        AttributeKind::Text { .. } | AttributeKind::ValueList { .. }
    );
    let ordered = matches!(
        kind,
        AttributeKind::Number { .. } | AttributeKind::Date | AttributeKind::Text { .. }
    );
    IndexFlags {
        searchable: text_or_list && index.is_multiple_of(2),
        filterable: index.is_multiple_of(3),
        sortable: ordered && !multiple && index % 2 == 1,
    }
}

fn capped(wanted: IndexFlags, budget: IndexBudget, used: &mut IndexBudget) -> IndexFlags {
    fn take(want: bool, cap: Option<usize>, used: &mut Option<usize>) -> bool {
        let count = used.get_or_insert(0);
        if !want || cap.is_some_and(|cap| *count >= cap) {
            return false;
        }
        *count += 1;
        true
    }
    IndexFlags {
        searchable: take(wanted.searchable, budget.searchable, &mut used.searchable),
        filterable: take(wanted.filterable, budget.filterable, &mut used.filterable),
        sortable: take(wanted.sortable, budget.sortable, &mut used.sortable),
    }
}

fn class(ids: &mut Ids, key: &str, group: GroupId, attributes: &[AttributeId]) -> Class {
    Class {
        id: ClassId::from_uuid(ids.next()),
        key: key.to_owned(),
        group,
        parent: None,
        attributes: attributes.to_vec(),
    }
}

fn label(language: &LanguageTag, text: String) -> BTreeMap<LanguageTag, String> {
    BTreeMap::from([(language.clone(), text)])
}
