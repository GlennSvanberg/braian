use std::collections::{HashMap, HashSet};

use rust_decimal::Decimal;
use serde_json::Value;

use openpim_model::{
    Attribute, AttributeId, AttributeKind, DefinitionSet, ErrorCode, GroupId, LanguageTag,
    NumberValue, Object, StructureId, UnitFamilyId, ValidationError,
};

use crate::compile::CompiledModel;
use crate::date::is_valid_date;

#[derive(Debug, Clone)]
pub(crate) struct ValueIndex {
    codes: HashMap<AttributeId, HashSet<String>>,
}

impl ValueIndex {
    pub(crate) fn build(defs: &DefinitionSet) -> Self {
        let codes = defs
            .attributes
            .iter()
            .filter_map(|attribute| {
                let AttributeKind::ValueList { options } = &attribute.kind else {
                    return None;
                };
                let codes = options.iter().map(|option| option.code.clone()).collect();
                Some((attribute.id, codes))
            })
            .collect();
        Self { codes }
    }
}

impl CompiledModel {
    pub fn validate(&self, object: &Object) -> Vec<ValidationError> {
        let mut errors = Vec::new();
        // Applicability depends on the type. Reporting every value as not
        // applicable would hide an unknown type.
        let Some(key_attribute) = self.key_attribute(object.type_id) else {
            errors.push(failure(
                None,
                None,
                None,
                ErrorCode::UnknownType,
                format!("unknown type {}", object.type_id),
            ));
            return errors;
        };
        let type_attributes = self
            .type_info(object.type_id)
            .expect("type was found above")
            .attributes
            .clone();

        let mut placed = Vec::new();
        for (structure_id, group_id) in &object.groups {
            match self.placement(*structure_id, *group_id) {
                Ok(()) => placed.push(*group_id),
                Err((code, message)) => errors.push(failure(None, None, None, code, message)),
            }
        }

        if !object.values.contains_key(&key_attribute) {
            let key = self
                .attribute(key_attribute)
                .map(|attribute| attribute.key.as_str())
                .unwrap_or("key");
            errors.push(failure(
                Some(key_attribute),
                None,
                None,
                ErrorCode::MissingKeyAttribute,
                format!("missing key attribute {key}"),
            ));
        }

        for (attribute_id, value) in &object.values {
            let Some(attribute) = self.attribute(*attribute_id) else {
                errors.push(failure(
                    Some(*attribute_id),
                    None,
                    None,
                    ErrorCode::AttributeNotApplicable,
                    format!("attribute {attribute_id} is not applicable"),
                ));
                continue;
            };
            if !self.applies(&type_attributes, &placed, *attribute_id) {
                errors.push(failure(
                    Some(*attribute_id),
                    None,
                    None,
                    ErrorCode::AttributeNotApplicable,
                    format!("attribute {} is not applicable", attribute.key),
                ));
                continue;
            }
            self.validate_attribute(attribute, value, &mut errors);
        }

        errors.sort_by(|left, right| {
            left.attribute
                .cmp(&right.attribute)
                .then(left.language.cmp(&right.language))
                .then(
                    left.index
                        .cmp(&right.index)
                        .then(left.code.cmp(&right.code))
                        .then(left.message.cmp(&right.message)),
                )
        });
        errors
    }

    fn placement(
        &self,
        structure_id: StructureId,
        group_id: GroupId,
    ) -> Result<(), (ErrorCode, String)> {
        if !self.has_structure(structure_id) {
            return Err((
                ErrorCode::UnknownStructure,
                format!("unknown structure {structure_id}"),
            ));
        }
        match self.group_info(group_id) {
            Some(group) if group.structure_id == structure_id => Ok(()),
            _ => Err((
                ErrorCode::UnknownGroup,
                format!("group {group_id} is not in structure {structure_id}"),
            )),
        }
    }

    fn applies(
        &self,
        type_attributes: &HashSet<AttributeId>,
        groups: &[GroupId],
        attribute: AttributeId,
    ) -> bool {
        type_attributes.contains(&attribute)
            || groups.iter().any(|group_id| {
                self.group_info(*group_id)
                    .is_some_and(|group| group.class_attributes.contains(&attribute))
            })
    }

    /// Entered number converted to the attribute's base unit.
    ///
    /// An attribute without a unit family returns the entered value unchanged.
    pub fn normalize(
        &self,
        attribute_id: AttributeId,
        value: &NumberValue,
    ) -> Result<Decimal, ValidationError> {
        let Some(attribute) = self.attribute(attribute_id) else {
            return Err(failure(
                Some(attribute_id),
                None,
                None,
                ErrorCode::AttributeNotApplicable,
                format!("attribute {attribute_id} is not applicable"),
            ));
        };
        let AttributeKind::Number { unit_family, .. } = &attribute.kind else {
            return Err(failure(
                Some(attribute_id),
                None,
                None,
                ErrorCode::WrongType,
                format!("attribute {} is not a number", attribute.key),
            ));
        };
        self.to_base(attribute, *unit_family, value, None, None)
    }

    fn validate_attribute(
        &self,
        attribute: &Attribute,
        value: &Value,
        errors: &mut Vec<ValidationError>,
    ) {
        if attribute.language_dependent {
            let Some(map) = value.as_object() else {
                errors.push(failure(
                    Some(attribute.id),
                    None,
                    None,
                    ErrorCode::ExpectedLanguageMap,
                    format!("attribute {} expected a language map", attribute.key),
                ));
                return;
            };
            for (raw, inner) in map {
                match LanguageTag::parse(raw) {
                    Ok(tag) if self.is_known_language(&tag) => {
                        self.validate_cardinality(attribute, inner, Some(&tag), errors);
                    }
                    Ok(tag) => errors.push(failure(
                        Some(attribute.id),
                        Some(tag),
                        None,
                        ErrorCode::LanguageNotAllowed,
                        format!(
                            "language {raw} is not allowed on attribute {}",
                            attribute.key
                        ),
                    )),
                    Err(_) => errors.push(failure(
                        Some(attribute.id),
                        None,
                        None,
                        ErrorCode::LanguageNotAllowed,
                        format!(
                            "language {raw} is not allowed on attribute {}",
                            attribute.key
                        ),
                    )),
                }
            }
            return;
        }
        self.validate_cardinality(attribute, value, None, errors);
    }

    fn validate_cardinality(
        &self,
        attribute: &Attribute,
        value: &Value,
        language: Option<&LanguageTag>,
        errors: &mut Vec<ValidationError>,
    ) {
        if attribute.multiple {
            let Some(items) = value.as_array() else {
                errors.push(failure(
                    Some(attribute.id),
                    language.cloned(),
                    None,
                    ErrorCode::ExpectedList,
                    format!("attribute {} expected a list", attribute.key),
                ));
                return;
            };
            for (index, item) in items.iter().enumerate() {
                self.validate_scalar(attribute, item, language, Some(index), errors);
            }
            return;
        }
        if value.is_array() {
            errors.push(failure(
                Some(attribute.id),
                language.cloned(),
                None,
                ErrorCode::WrongType,
                format!("attribute {} expected a single value", attribute.key),
            ));
            return;
        }
        self.validate_scalar(attribute, value, language, None, errors);
    }

    fn validate_scalar(
        &self,
        attribute: &Attribute,
        value: &Value,
        language: Option<&LanguageTag>,
        index: Option<usize>,
        errors: &mut Vec<ValidationError>,
    ) {
        match &attribute.kind {
            AttributeKind::Text { max_length } => {
                let Some(text) = value.as_str() else {
                    errors.push(wrong_type(attribute, language, index, "a string"));
                    return;
                };
                if let Some(max) = max_length {
                    let length = text.chars().count();
                    if length > *max as usize {
                        errors.push(failure(
                            Some(attribute.id),
                            language.cloned(),
                            index,
                            ErrorCode::TextTooLong,
                            format!(
                                "attribute {} is {length} characters, max is {max}",
                                attribute.key
                            ),
                        ));
                    }
                }
            }
            AttributeKind::Number {
                unit_family,
                min,
                max,
            } => {
                let parsed = match parse_number(value) {
                    Ok(parsed) => parsed,
                    Err(code) => {
                        let message = match code {
                            ErrorCode::InvalidDecimal => "invalid decimal".to_owned(),
                            _ => "expected a number object".to_owned(),
                        };
                        errors.push(failure(
                            Some(attribute.id),
                            language.cloned(),
                            index,
                            code,
                            format!("attribute {}: {message}", attribute.key),
                        ));
                        return;
                    }
                };
                let normalized =
                    match self.to_base(attribute, *unit_family, &parsed, language, index) {
                        Ok(normalized) => normalized,
                        Err(error) => {
                            errors.push(error);
                            return;
                        }
                    };
                if let Some(min) = min
                    && normalized < *min
                {
                    errors.push(failure(
                        Some(attribute.id),
                        language.cloned(),
                        index,
                        ErrorCode::BelowMin,
                        format!("attribute {} is below min {min}", attribute.key),
                    ));
                    return;
                }
                if let Some(max) = max
                    && normalized > *max
                {
                    errors.push(failure(
                        Some(attribute.id),
                        language.cloned(),
                        index,
                        ErrorCode::AboveMax,
                        format!("attribute {} is above max {max}", attribute.key),
                    ));
                }
            }
            AttributeKind::ValueList { .. } => {
                let Some(code) = value.as_str() else {
                    errors.push(wrong_type(attribute, language, index, "a value-list code"));
                    return;
                };
                let allowed = self.value_index().codes.get(&attribute.id);
                if allowed.is_none_or(|codes| !codes.contains(code)) {
                    errors.push(failure(
                        Some(attribute.id),
                        language.cloned(),
                        index,
                        ErrorCode::ValueNotInList,
                        format!(
                            "value {code} is not in the list for attribute {}",
                            attribute.key
                        ),
                    ));
                }
            }
            AttributeKind::Boolean => {
                if !value.is_boolean() {
                    errors.push(wrong_type(attribute, language, index, "a boolean"));
                }
            }
            AttributeKind::Date => {
                let Some(text) = value.as_str() else {
                    errors.push(wrong_type(attribute, language, index, "a date"));
                    return;
                };
                if !is_valid_date(text) {
                    errors.push(failure(
                        Some(attribute.id),
                        language.cloned(),
                        index,
                        ErrorCode::InvalidDate,
                        format!("attribute {} has invalid date {text}", attribute.key),
                    ));
                }
            }
            AttributeKind::FileRef => {
                let Some(text) = value.as_str() else {
                    errors.push(wrong_type(attribute, language, index, "a file reference"));
                    return;
                };
                if openpim_model::Uuid::parse_str(text).is_err() {
                    errors.push(failure(
                        Some(attribute.id),
                        language.cloned(),
                        index,
                        ErrorCode::InvalidFileRef,
                        format!("attribute {} has invalid file reference", attribute.key),
                    ));
                }
            }
        }
    }

    fn to_base(
        &self,
        attribute: &Attribute,
        unit_family: Option<UnitFamilyId>,
        value: &NumberValue,
        language: Option<&LanguageTag>,
        index: Option<usize>,
    ) -> Result<Decimal, ValidationError> {
        let Some(family) = unit_family else {
            if value.unit.is_some() {
                return Err(failure(
                    Some(attribute.id),
                    language.cloned(),
                    index,
                    ErrorCode::UnitNotAllowed,
                    format!("attribute {} does not take a unit", attribute.key),
                ));
            }
            return Ok(value.value);
        };
        let Some(unit) = value.unit.as_deref() else {
            return Err(failure(
                Some(attribute.id),
                language.cloned(),
                index,
                ErrorCode::UnitRequired,
                format!("attribute {} requires a unit", attribute.key),
            ));
        };
        let Some(factor) = self.unit_factor(family, unit) else {
            return Err(failure(
                Some(attribute.id),
                language.cloned(),
                index,
                ErrorCode::UnitNotAllowed,
                format!("unit {unit} is not allowed on attribute {}", attribute.key),
            ));
        };
        value.value.checked_mul(factor).ok_or_else(|| {
            failure(
                Some(attribute.id),
                language.cloned(),
                index,
                ErrorCode::InvalidDecimal,
                format!(
                    "attribute {} overflowed when converting units",
                    attribute.key
                ),
            )
        })
    }
}

fn parse_number(value: &Value) -> Result<NumberValue, ErrorCode> {
    let Some(object) = value.as_object() else {
        return Err(ErrorCode::WrongType);
    };
    if object.len() > 2 || object.keys().any(|key| key != "value" && key != "unit") {
        return Err(ErrorCode::WrongType);
    }
    let Some(raw) = object.get("value") else {
        return Err(ErrorCode::InvalidDecimal);
    };
    let Some(text) = raw.as_str() else {
        return Err(ErrorCode::InvalidDecimal);
    };
    let Ok(parsed) = text.parse::<Decimal>() else {
        return Err(ErrorCode::InvalidDecimal);
    };
    let unit = match object.get("unit") {
        None => None,
        Some(Value::String(unit)) => Some(unit.clone()),
        Some(_) => return Err(ErrorCode::WrongType),
    };
    Ok(NumberValue {
        value: parsed,
        unit,
    })
}

fn wrong_type(
    attribute: &Attribute,
    language: Option<&LanguageTag>,
    index: Option<usize>,
    expected: &str,
) -> ValidationError {
    failure(
        Some(attribute.id),
        language.cloned(),
        index,
        ErrorCode::WrongType,
        format!("attribute {} expected {expected}", attribute.key),
    )
}

fn failure(
    attribute: Option<AttributeId>,
    language: Option<LanguageTag>,
    index: Option<usize>,
    code: ErrorCode,
    message: String,
) -> ValidationError {
    ValidationError {
        attribute,
        language,
        index,
        code,
        message,
    }
}
