use std::collections::HashSet;

use braian_model::{
    Attribute, AttributeId, AttributeKind, DefinitionSet, ErrorCode, GroupId, LanguageTag, Object,
    StructureId, TypeId, UnitFamilyId,
};
use braian_rules::CompiledModel;
use rand::RngExt;
use rust_decimal::Decimal;
use serde_json::{Map, Value};
use uuid::Uuid;

/// Where a mutation is expected to fail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedError {
    pub attribute: Option<AttributeId>,
    pub language: Option<LanguageTag>,
    pub index: Option<usize>,
    pub code: ErrorCode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mutation {
    UnknownType,
    UnknownStructure,
    UnknownGroup,
    AttributeNotApplicable,
    UnknownAttribute,
    WrongType,
    ExpectedList,
    ExpectedLanguageMap,
    LanguageNotAllowed,
    TextTooLong,
    InvalidDecimal,
    UnitRequired,
    UnitNotAllowed,
    UnitOnUnitless,
    BelowMin,
    AboveMax,
    ValueNotInList,
    InvalidDate,
    InvalidFileRef,
    MissingKey,
}

impl Mutation {
    pub const ALL: [Self; 20] = [
        Self::UnknownType,
        Self::UnknownStructure,
        Self::UnknownGroup,
        Self::AttributeNotApplicable,
        Self::UnknownAttribute,
        Self::WrongType,
        Self::ExpectedList,
        Self::ExpectedLanguageMap,
        Self::LanguageNotAllowed,
        Self::TextTooLong,
        Self::InvalidDecimal,
        Self::UnitRequired,
        Self::UnitNotAllowed,
        Self::UnitOnUnitless,
        Self::BelowMin,
        Self::AboveMax,
        Self::ValueNotInList,
        Self::InvalidDate,
        Self::InvalidFileRef,
        Self::MissingKey,
    ];
}

pub fn mutate(
    object: &Object,
    defs: &DefinitionSet,
    rng: &mut impl RngExt,
) -> (Object, ExpectedError) {
    let all = all_mutations(object, defs);
    let index = rng.random_range(0..all.len());
    all.into_iter().nth(index).expect("mutation index")
}

pub fn all_mutations(object: &Object, defs: &DefinitionSet) -> Vec<(Object, ExpectedError)> {
    let model = CompiledModel::compile(defs).expect("definitions compile");
    Mutation::ALL
        .iter()
        .map(|mutation| apply(&model, defs, object, *mutation))
        .collect()
}

pub fn apply_mutation(
    object: &Object,
    defs: &DefinitionSet,
    mutation: Mutation,
) -> (Object, ExpectedError) {
    let model = CompiledModel::compile(defs).expect("definitions compile");
    apply(&model, defs, object, mutation)
}

fn apply(
    model: &CompiledModel,
    defs: &DefinitionSet,
    object: &Object,
    mutation: Mutation,
) -> (Object, ExpectedError) {
    let language = model.languages().first().cloned();
    let mut broken = object.clone();
    let expected = match mutation {
        Mutation::UnknownType => {
            broken.type_id = TypeId::from_uuid(Uuid::nil());
            located(None, None, None, ErrorCode::UnknownType)
        }
        Mutation::UnknownStructure => {
            broken.groups.insert(
                StructureId::from_uuid(Uuid::nil()),
                GroupId::from_uuid(Uuid::from_u128(1)),
            );
            located(None, None, None, ErrorCode::UnknownStructure)
        }
        Mutation::UnknownGroup => {
            let channel = model.structure_id("channel").expect("channel");
            let mut without = object.groups.clone();
            without.remove(&channel);
            let still: HashSet<_> = model
                .applicable_ids(object.type_id, &without)
                .expect("catalog placement")
                .into_iter()
                .collect();
            broken.values.retain(|id, _| still.contains(id));
            broken
                .groups
                .insert(channel, GroupId::from_uuid(Uuid::nil()));
            located(None, None, None, ErrorCode::UnknownGroup)
        }
        Mutation::AttributeNotApplicable => {
            let unused = model.attribute_by_key("unused").expect("unused attribute");
            broken
                .values
                .insert(unused.id, Value::String("x".to_owned()));
            located(
                Some(unused.id),
                None,
                None,
                ErrorCode::AttributeNotApplicable,
            )
        }
        Mutation::UnknownAttribute => {
            let id = AttributeId::from_uuid(Uuid::from_u128(7));
            broken.values.insert(id, Value::String("x".to_owned()));
            located(Some(id), None, None, ErrorCode::AttributeNotApplicable)
        }
        Mutation::WrongType => {
            let key = model.key_attribute(object.type_id).expect("key");
            broken.values.insert(key, Value::Bool(true));
            located(Some(key), None, None, ErrorCode::WrongType)
        }
        Mutation::ExpectedList => {
            let attribute = find(model, object, |attribute| {
                attribute.multiple && !attribute.language_dependent
            });
            broken
                .values
                .insert(attribute.id, Value::String("x".to_owned()));
            located(Some(attribute.id), None, None, ErrorCode::ExpectedList)
        }
        Mutation::ExpectedLanguageMap => {
            let attribute = find(model, object, |attribute| attribute.language_dependent);
            broken
                .values
                .insert(attribute.id, Value::String("x".to_owned()));
            located(
                Some(attribute.id),
                None,
                None,
                ErrorCode::ExpectedLanguageMap,
            )
        }
        Mutation::LanguageNotAllowed => {
            let attribute = find(model, object, |attribute| attribute.language_dependent);
            let tag = LanguageTag::parse("zu").expect("tag");
            let mut map = Map::new();
            map.insert(tag.as_str().to_owned(), Value::String("x".to_owned()));
            broken.values.insert(attribute.id, Value::Object(map));
            located(
                Some(attribute.id),
                Some(tag),
                None,
                ErrorCode::LanguageNotAllowed,
            )
        }
        Mutation::TextTooLong => {
            let attribute = find(model, object, |attribute| {
                matches!(
                    attribute.kind,
                    AttributeKind::Text {
                        max_length: Some(_),
                        ..
                    }
                ) && !attribute.multiple
                    && !attribute.language_dependent
                    && attribute.key != "sku"
            });
            let AttributeKind::Text {
                max_length: Some(max),
                ..
            } = attribute.kind
            else {
                unreachable!("predicate checked the text kind");
            };
            let (value, tag, index) = place(
                &attribute,
                Value::String("a".repeat(max as usize + 1)),
                language.as_ref(),
            );
            broken.values.insert(attribute.id, value);
            located(Some(attribute.id), tag, index, ErrorCode::TextTooLong)
        }
        Mutation::InvalidDecimal => {
            let attribute = find(model, object, |attribute| {
                matches!(attribute.kind, AttributeKind::Number { .. })
            });
            let (value, tag, index) = place(&attribute, number("nope", None), language.as_ref());
            broken.values.insert(attribute.id, value);
            located(Some(attribute.id), tag, index, ErrorCode::InvalidDecimal)
        }
        Mutation::UnitRequired => {
            let attribute = find(model, object, number_with_unit);
            let (value, tag, index) = place(&attribute, number("1", None), language.as_ref());
            broken.values.insert(attribute.id, value);
            located(Some(attribute.id), tag, index, ErrorCode::UnitRequired)
        }
        Mutation::UnitNotAllowed => {
            let attribute = find(model, object, number_with_unit);
            let (value, tag, index) =
                place(&attribute, number("1", Some("furlong")), language.as_ref());
            broken.values.insert(attribute.id, value);
            located(Some(attribute.id), tag, index, ErrorCode::UnitNotAllowed)
        }
        Mutation::UnitOnUnitless => {
            let attribute = find(model, object, |attribute| {
                matches!(
                    attribute.kind,
                    AttributeKind::Number {
                        unit_family: None,
                        ..
                    }
                )
            });
            let (value, tag, index) = place(&attribute, number("1", Some("kg")), language.as_ref());
            broken.values.insert(attribute.id, value);
            located(Some(attribute.id), tag, index, ErrorCode::UnitNotAllowed)
        }
        Mutation::BelowMin => {
            let attribute = find(model, object, |attribute| {
                matches!(attribute.kind, AttributeKind::Number { min: Some(_), .. })
            });
            let AttributeKind::Number {
                min: Some(min),
                unit_family,
                ..
            } = &attribute.kind
            else {
                unreachable!("predicate checked min");
            };
            let under = min - Decimal::ONE;
            let unit = unit_family.map(|family| base_unit(defs, family));
            let (value, tag, index) = place(
                &attribute,
                number(&under.to_string(), unit.as_deref()),
                language.as_ref(),
            );
            broken.values.insert(attribute.id, value);
            located(Some(attribute.id), tag, index, ErrorCode::BelowMin)
        }
        Mutation::AboveMax => {
            let attribute = find(model, object, |attribute| {
                matches!(attribute.kind, AttributeKind::Number { max: Some(_), .. })
            });
            let AttributeKind::Number {
                max: Some(max),
                unit_family,
                ..
            } = &attribute.kind
            else {
                unreachable!("predicate checked max");
            };
            let over = max + Decimal::ONE;
            let unit = unit_family.map(|family| base_unit(defs, family));
            let (value, tag, index) = place(
                &attribute,
                number(&over.to_string(), unit.as_deref()),
                language.as_ref(),
            );
            broken.values.insert(attribute.id, value);
            located(Some(attribute.id), tag, index, ErrorCode::AboveMax)
        }
        Mutation::ValueNotInList => {
            let attribute = find(model, object, |attribute| {
                matches!(attribute.kind, AttributeKind::ValueList { .. })
            });
            let (value, tag, index) = place(
                &attribute,
                Value::String("no-such".to_owned()),
                language.as_ref(),
            );
            broken.values.insert(attribute.id, value);
            located(Some(attribute.id), tag, index, ErrorCode::ValueNotInList)
        }
        Mutation::InvalidDate => {
            let attribute = find(model, object, |attribute| {
                matches!(attribute.kind, AttributeKind::Date)
            });
            let (value, tag, index) = place(
                &attribute,
                Value::String("2020-13-01".to_owned()),
                language.as_ref(),
            );
            broken.values.insert(attribute.id, value);
            located(Some(attribute.id), tag, index, ErrorCode::InvalidDate)
        }
        Mutation::InvalidFileRef => {
            let attribute = find(model, object, |attribute| {
                matches!(attribute.kind, AttributeKind::FileRef { .. })
            });
            let (value, tag, index) = place(
                &attribute,
                Value::String("not-a-uuid".to_owned()),
                language.as_ref(),
            );
            broken.values.insert(attribute.id, value);
            located(Some(attribute.id), tag, index, ErrorCode::InvalidFileRef)
        }
        Mutation::MissingKey => {
            let key = model.key_attribute(object.type_id).expect("key");
            broken.values.remove(&key);
            located(Some(key), None, None, ErrorCode::MissingKeyAttribute)
        }
    };
    (broken, expected)
}

fn find(
    model: &CompiledModel,
    object: &Object,
    mut pred: impl FnMut(&Attribute) -> bool,
) -> Attribute {
    let ids = model
        .applicable_ids(object.type_id, &object.groups)
        .expect("placement");
    ids.iter()
        .find_map(|id| {
            let attribute = model.attribute(*id).expect("attribute");
            pred(attribute).then(|| attribute.clone())
        })
        .expect("synthetic tier includes this attribute")
}

fn number_with_unit(attribute: &Attribute) -> bool {
    matches!(
        attribute.kind,
        AttributeKind::Number {
            unit_family: Some(_),
            ..
        }
    )
}

fn base_unit(defs: &DefinitionSet, family: UnitFamilyId) -> String {
    defs.unit_families
        .iter()
        .find(|item| item.id == family)
        .expect("unit family")
        .base_unit
        .clone()
}

/// Puts `scalar` where the attribute expects a single value, remembering the location.
fn place(
    attribute: &Attribute,
    scalar: Value,
    language: Option<&LanguageTag>,
) -> (Value, Option<LanguageTag>, Option<usize>) {
    let (value, index) = if attribute.multiple {
        (Value::Array(vec![scalar]), Some(0))
    } else {
        (scalar, None)
    };
    if attribute.language_dependent {
        let language = language.expect("language").clone();
        let mut map = Map::new();
        map.insert(language.as_str().to_owned(), value);
        (Value::Object(map), Some(language), index)
    } else {
        (value, None, index)
    }
}

fn number(value: &str, unit: Option<&str>) -> Value {
    let mut map = Map::new();
    map.insert("value".to_owned(), Value::String(value.to_owned()));
    if let Some(unit) = unit {
        map.insert("unit".to_owned(), Value::String(unit.to_owned()));
    }
    Value::Object(map)
}

fn located(
    attribute: Option<AttributeId>,
    language: Option<LanguageTag>,
    index: Option<usize>,
    code: ErrorCode,
) -> ExpectedError {
    ExpectedError {
        attribute,
        language,
        index,
        code,
    }
}
