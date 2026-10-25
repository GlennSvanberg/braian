//! Cell parsing and row classification for the import preview.

use std::collections::{BTreeMap, HashMap};

use braian_model::{
    Attribute, AttributeId, AttributeKind, AttributePatch, DefinitionSet, ErrorCode, LanguageTag,
    Object, ObjectId, ObjectPatch, TypeId, ValuePatch,
};
use braian_rules::CompiledModel;
use braian_store::StoredObject;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use super::mapping::{Target, find_target};

pub const DIFF_CHUNK: usize = 2_000;

#[derive(Debug, Clone)]
pub struct MappedColumn {
    pub index: usize,
    pub target: String,
}

#[derive(Debug, Clone)]
pub struct RowOutcome {
    pub status: String,
    pub object_id: Option<Uuid>,
    pub object_key: Option<String>,
    pub base_version: Option<i64>,
    pub cells: Value,
    pub patch: Option<Value>,
    pub errors: Value,
}

pub struct Classified {
    pub outcome: RowOutcome,
    #[allow(dead_code)]
    pub key: Option<String>,
}

#[allow(clippy::too_many_arguments)]
pub fn classify_row(
    model: &CompiledModel,
    defs: &DefinitionSet,
    targets: &[Target],
    type_key: &str,
    columns: &[MappedColumn],
    id_column: Option<usize>,
    row_no: u32,
    cells: &[Option<String>],
    existing: Option<&StoredObject>,
    duplicate_of: Option<u32>,
    resolve_ref: &mut dyn FnMut(&str, &str) -> Option<Uuid>,
) -> Classified {
    let Some(type_id) = model.type_id(type_key) else {
        return failed_row(
            row_no,
            cells,
            "unknown_type",
            "That object type does not exist.",
        );
    };
    if let Some(first) = duplicate_of {
        return failed_row(
            row_no,
            cells,
            "duplicate_id",
            &format!("Same id as row {first}"),
        );
    }
    let mut cell_view = Map::new();
    let mut errors = Vec::new();
    let mut values: BTreeMap<AttributeId, Value> = BTreeMap::new();
    let mut languages: BTreeMap<AttributeId, BTreeMap<LanguageTag, Value>> = BTreeMap::new();
    let mut groups = BTreeMap::new();
    let mut action = String::new();
    for column in columns {
        if column.target == "unmapped" {
            continue;
        }
        let raw = cells.get(column.index).and_then(|cell| cell.clone());
        let Some(target) = find_target(targets, &column.target) else {
            cell_view.insert(
                column.index.to_string(),
                json!({ "value": raw, "state": "error", "error": { "code": "unknown_target", "message": "That field is not in the data model." } }),
            );
            errors.push(json!({ "code": "unknown_target", "message": "That field is not in the data model." }));
            continue;
        };
        if target.id == "action" {
            action = raw.clone().unwrap_or_default().trim().to_lowercase();
            cell_view.insert(
                column.index.to_string(),
                json!({ "value": raw.clone().unwrap_or_default(), "state": "same" }),
            );
            continue;
        }
        let Some(text) = raw else {
            cell_view.insert(
                column.index.to_string(),
                json!({ "value": "", "state": "same" }),
            );
            continue;
        };
        if text == "#CLEAR" {
            let error = json!({ "code": "unsupported_clear", "message": "Clearing a value is not supported." });
            cell_view.insert(
                column.index.to_string(),
                json!({ "value": text, "state": "error", "error": error }),
            );
            errors.push(error);
            continue;
        }
        match apply_cell(
            model,
            defs,
            target,
            &text,
            &mut values,
            &mut languages,
            &mut groups,
            resolve_ref,
        ) {
            Ok(()) => {
                cell_view.insert(
                    column.index.to_string(),
                    json!({ "value": text, "state": "new" }),
                );
            }
            Err(error) => {
                cell_view.insert(
                    column.index.to_string(),
                    json!({ "value": text, "state": "error", "error": error }),
                );
                errors.push(error);
            }
        }
    }
    merge_languages(&mut values, languages);
    if !action.is_empty()
        && action != "upsert"
        && action != "update"
        && action != "create"
        && action != "delete"
    {
        errors.push(
            json!({ "code": "invalid_action", "message": format!("Unknown action {action}.") }),
        );
    }
    if let Some(existing) = existing {
        paint_changes(
            &mut cell_view,
            model,
            columns,
            targets,
            &existing.object.values,
            &values,
        );
    }
    let key = id_column.and_then(|index| {
        cells
            .get(index)
            .and_then(|cell| cell.clone())
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty())
    });
    if !errors.is_empty() && action != "delete" {
        return Classified {
            key: key.clone(),
            outcome: RowOutcome {
                status: "failed".into(),
                object_id: existing.map(|item| item.object.id.as_uuid()),
                object_key: key,
                base_version: existing.map(|item| item.version as i64),
                cells: Value::Object(cell_view),
                patch: None,
                errors: Value::Array(errors),
            },
        };
    }
    if action == "delete" {
        let Some(existing) = existing else {
            return failed_row(
                row_no,
                cells,
                "delete_not_found",
                "Delete needs an existing object.",
            );
        };
        return Classified {
            key: key.clone(),
            outcome: RowOutcome {
                status: "delete".into(),
                object_id: Some(existing.object.id.as_uuid()),
                object_key: key.or(Some(existing.key.clone())),
                base_version: Some(existing.version as i64),
                cells: Value::Object(cell_view),
                patch: None,
                errors: Value::Array(errors),
            },
        };
    }
    if let Some(existing) = existing {
        let patch = diff_patch(
            model,
            &existing.object.values,
            &values,
            &groups,
            &existing.object.groups,
        );
        if patch.is_empty() {
            return Classified {
                key,
                outcome: RowOutcome {
                    status: "unchanged".into(),
                    object_id: Some(existing.object.id.as_uuid()),
                    object_key: Some(existing.key.clone()),
                    base_version: Some(existing.version as i64),
                    cells: Value::Object(cell_view),
                    patch: None,
                    errors: Value::Array(Vec::new()),
                },
            };
        }
        let report = model.validate_patch(&existing.object, &patch);
        if !report.errors.is_empty() {
            attach_validation(&mut cell_view, model, columns, targets, &report.errors);
            return Classified {
                key: key.or(Some(existing.key.clone())),
                outcome: RowOutcome {
                    status: "failed".into(),
                    object_id: Some(existing.object.id.as_uuid()),
                    object_key: Some(existing.key.clone()),
                    base_version: Some(existing.version as i64),
                    cells: Value::Object(cell_view),
                    patch: None,
                    errors: Value::Array(report.errors.iter().map(error_json).collect()),
                },
            };
        }
        return Classified {
            key: key.or(Some(existing.key.clone())),
            outcome: RowOutcome {
                status: "changed".into(),
                object_id: Some(existing.object.id.as_uuid()),
                object_key: Some(existing.key.clone()),
                base_version: Some(existing.version as i64),
                cells: Value::Object(cell_view),
                patch: serde_json::to_value(&patch).ok(),
                errors: Value::Array(Vec::new()),
            },
        };
    }
    let object = Object {
        id: ObjectId::from_uuid(Uuid::now_v7()),
        type_id,
        model_revision: defs.model_revision,
        groups,
        values,
    };
    let validation = model.validate(&object);
    if !validation.is_empty() {
        attach_validation(&mut cell_view, model, columns, targets, &validation);
        return Classified {
            key,
            outcome: RowOutcome {
                status: "failed".into(),
                object_id: None,
                object_key: None,
                base_version: None,
                cells: Value::Object(cell_view),
                patch: None,
                errors: Value::Array(validation.iter().map(error_json).collect()),
            },
        };
    }
    let object_key = key.or_else(|| object_key_value(model, type_id, &object));
    Classified {
        key: object_key.clone(),
        outcome: RowOutcome {
            status: "new".into(),
            object_id: Some(object.id.as_uuid()),
            object_key,
            base_version: None,
            cells: Value::Object(cell_view),
            patch: serde_json::to_value(&object).ok(),
            errors: Value::Array(Vec::new()),
        },
    }
}

fn failed_row(row_no: u32, cells: &[Option<String>], code: &str, message: &str) -> Classified {
    let _ = row_no;
    let mut view = Map::new();
    for (index, cell) in cells.iter().enumerate() {
        view.insert(
            index.to_string(),
            json!({ "value": cell.clone().unwrap_or_default(), "state": "same" }),
        );
    }
    Classified {
        key: None,
        outcome: RowOutcome {
            status: "failed".into(),
            object_id: None,
            object_key: None,
            base_version: None,
            cells: Value::Object(view),
            patch: None,
            errors: json!([{ "code": code, "message": message }]),
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_cell(
    model: &CompiledModel,
    defs: &DefinitionSet,
    target: &Target,
    text: &str,
    values: &mut BTreeMap<AttributeId, Value>,
    languages: &mut BTreeMap<AttributeId, BTreeMap<LanguageTag, Value>>,
    groups: &mut BTreeMap<braian_model::StructureId, braian_model::GroupId>,
    resolve_ref: &mut dyn FnMut(&str, &str) -> Option<Uuid>,
) -> Result<(), Value> {
    if target.id.starts_with("membership:") {
        let Some(structure_key) = target.structure_key.as_deref() else {
            return Err(err("unknown_structure", "That structure does not exist."));
        };
        let Some(structure_id) = model.structure_id(structure_key) else {
            return Err(err("unknown_structure", "That structure does not exist."));
        };
        let Some(group_id) = model.group_id(structure_key, text.trim()) else {
            return Err(err("unknown_group", "That group does not exist."));
        };
        groups.insert(structure_id, group_id);
        return Ok(());
    }
    let Some(attribute_key) = target.attribute_key.as_deref() else {
        return Err(err(
            "unknown_target",
            "That field is not in the data model.",
        ));
    };
    let Some(attribute) = model.attribute_by_key(attribute_key) else {
        return Err(err(
            "unknown_target",
            "That field is not in the data model.",
        ));
    };
    let parsed = parse_cell(
        defs,
        attribute,
        target.language.as_deref(),
        text,
        resolve_ref,
    )?;
    if let Some(language) = &target.language {
        let tag = LanguageTag::parse(language)
            .map_err(|_| err("invalid_language", "That language is not allowed."))?;
        languages
            .entry(attribute.id)
            .or_default()
            .insert(tag, parsed);
    } else {
        values.insert(attribute.id, parsed);
    }
    Ok(())
}

fn merge_languages(
    values: &mut BTreeMap<AttributeId, Value>,
    languages: BTreeMap<AttributeId, BTreeMap<LanguageTag, Value>>,
) {
    for (id, map) in languages {
        let mut object = Map::new();
        if let Some(Value::Object(existing)) = values.get(&id) {
            object = existing.clone();
        }
        for (language, value) in map {
            object.insert(language.as_str().to_owned(), value);
        }
        values.insert(id, Value::Object(object));
    }
}

pub fn parse_cell(
    defs: &DefinitionSet,
    attribute: &Attribute,
    language: Option<&str>,
    text: &str,
    resolve_ref: &mut dyn FnMut(&str, &str) -> Option<Uuid>,
) -> Result<Value, Value> {
    let _ = language;
    if attribute.multiple {
        let parts: Vec<&str> = text
            .split([';', '|'])
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect();
        let mut items = Vec::new();
        for part in parts {
            items.push(parse_scalar(defs, attribute, part, resolve_ref)?);
        }
        return Ok(Value::Array(items));
    }
    parse_scalar(defs, attribute, text.trim(), resolve_ref)
}

fn parse_scalar(
    defs: &DefinitionSet,
    attribute: &Attribute,
    text: &str,
    resolve_ref: &mut dyn FnMut(&str, &str) -> Option<Uuid>,
) -> Result<Value, Value> {
    match &attribute.kind {
        AttributeKind::Text { .. } | AttributeKind::FileRef { .. } => {
            Ok(Value::String(text.to_owned()))
        }
        AttributeKind::Boolean => parse_bool(text).map(Value::Bool),
        AttributeKind::Date => parse_date(text).map(Value::String),
        AttributeKind::Number { unit_family, .. } => {
            let (number, unit) = parse_number_text(text)?;
            let unit = match (unit, unit_family) {
                (Some(unit), Some(_)) => Some(unit),
                (None, Some(family)) => defs
                    .unit_families
                    .iter()
                    .find(|item| item.id == *family)
                    .map(|item| item.base_unit.clone()),
                (Some(unit), None) => Some(unit),
                (None, None) => None,
            };
            let mut object = Map::new();
            object.insert("value".into(), Value::String(number));
            if let Some(unit) = unit {
                object.insert("unit".into(), Value::String(unit));
            }
            Ok(Value::Object(object))
        }
        AttributeKind::Range { .. } => parse_range(text),
        AttributeKind::ValueList { options, list } => {
            let code = match_option(defs, options, list.as_deref(), text)?;
            Ok(Value::String(code))
        }
        AttributeKind::ObjectRef { type_key } => {
            if Uuid::parse_str(text).is_ok() {
                return Ok(Value::String(text.to_owned()));
            }
            let Some(type_key) = type_key.as_deref() else {
                return Err(err("invalid_object_ref", "That reference has no type."));
            };
            match resolve_ref(type_key, text) {
                Some(id) => Ok(Value::String(id.to_string())),
                None => Err(err("invalid_object_ref", "That object was not found.")),
            }
        }
    }
}

pub fn parse_number_text(text: &str) -> Result<(String, Option<String>), Value> {
    let trimmed = text.trim();
    let (numeric, unit) = split_unit(trimmed);
    let canonical =
        canonicalize_number(&numeric).ok_or_else(|| err("invalid_decimal", "Not a number"))?;
    Ok((canonical, unit))
}

fn split_unit(text: &str) -> (String, Option<String>) {
    let mut chars = text.chars().peekable();
    let mut numeric = String::new();
    while let Some(character) = chars.peek().copied() {
        if character.is_ascii_digit()
            || matches!(character, '+' | '-' | '.' | ',' | ' ' | '\u{00a0}')
        {
            numeric.push(character);
            chars.next();
        } else {
            break;
        }
    }
    let rest: String = chars.collect();
    let rest = rest.trim();
    if rest.is_empty() {
        (numeric, None)
    } else {
        (numeric, Some(rest.to_owned()))
    }
}

fn canonicalize_number(text: &str) -> Option<String> {
    let compact: String = text
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    if compact.is_empty() || !compact.chars().any(|character| character.is_ascii_digit()) {
        return None;
    }
    let (sign, body) = if let Some(rest) = compact.strip_prefix('+') {
        ("", rest)
    } else if let Some(rest) = compact.strip_prefix('-') {
        ("-", rest)
    } else {
        ("", compact.as_str())
    };
    let comma = body.rfind(',');
    let dot = body.rfind('.');
    let (int_part, frac) = match (comma, dot) {
        (Some(comma_at), Some(dot_at)) => {
            let decimal_at = comma_at.max(dot_at);
            let decimal = if body[decimal_at..].chars().nth(1).is_some() {
                &body[decimal_at + 1..]
            } else {
                return None;
            };
            if !(1..=3).contains(&decimal.len())
                || !decimal.chars().all(|character| character.is_ascii_digit())
            {
                return None;
            }
            (&body[..decimal_at], Some(decimal))
        }
        (Some(at), None) | (None, Some(at)) => {
            let separator = body.as_bytes()[at] as char;
            let groups: Vec<&str> = body.split(separator).collect();
            let grouped = groups.len() > 1
                && groups.iter().skip(1).all(|group| {
                    group.len() == 3 && group.chars().all(|character| character.is_ascii_digit())
                })
                && groups[0]
                    .chars()
                    .all(|character| character.is_ascii_digit());
            if grouped && (groups.len() > 2 || groups[1].len() == 3) {
                let digits: String = body
                    .chars()
                    .filter(|character| character.is_ascii_digit())
                    .collect();
                return Some(format!("{sign}{digits}"));
            }
            let decimal = &body[at + 1..];
            if decimal.chars().all(|character| character.is_ascii_digit()) {
                (&body[..at], Some(decimal))
            } else {
                return None;
            }
        }
        (None, None) => (body, None),
    };
    let int_digits: String = int_part
        .chars()
        .filter(|character| character.is_ascii_digit())
        .collect();
    if int_digits.is_empty() && frac.is_none() {
        return None;
    }
    let int_digits = if int_digits.is_empty() {
        "0".to_owned()
    } else {
        int_digits
    };
    let mut out = format!("{sign}{int_digits}");
    if let Some(frac) = frac {
        let frac: String = frac
            .chars()
            .filter(|character| character.is_ascii_digit())
            .collect();
        if !frac.is_empty() {
            out.push('.');
            out.push_str(&frac);
        }
    }
    Some(out)
}

fn parse_bool(text: &str) -> Result<bool, Value> {
    match text.trim().to_lowercase().as_str() {
        "true" | "yes" | "ja" | "1" | "x" => Ok(true),
        "false" | "no" | "nej" | "0" => Ok(false),
        _ => Err(err("invalid_boolean", "Not a boolean")),
    }
}

fn parse_date(text: &str) -> Result<String, Value> {
    let text = text.trim();
    let candidate = if let Some(iso) = text.get(0..10)
        && text.len() >= 10
        && iso.as_bytes().get(4) == Some(&b'-')
        && iso.as_bytes().get(7) == Some(&b'-')
        && (text.len() == 10 || matches!(text.as_bytes().get(10), Some(b'T' | b' ')))
    {
        iso.to_owned()
    } else {
        let parts: Vec<&str> = text.split(['/', '-', '.']).collect();
        if parts.len() == 3
            && parts
                .iter()
                .all(|part| part.chars().all(|character| character.is_ascii_digit()))
        {
            if parts[0].len() == 4 {
                format!("{:0>4}-{:0>2}-{:0>2}", parts[0], parts[1], parts[2])
            } else if parts[2].len() == 4 {
                format!("{:0>4}-{:0>2}-{:0>2}", parts[2], parts[1], parts[0])
            } else {
                return Err(err("invalid_date", "Not a date"));
            }
        } else {
            return Err(err("invalid_date", "Not a date"));
        }
    };
    if !calendar_date(&candidate) {
        return Err(err("invalid_date", "Not a date"));
    }
    Ok(candidate)
}

fn calendar_date(iso: &str) -> bool {
    let bytes = iso.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    let Some(year) = parse_u32(&bytes[0..4]) else {
        return false;
    };
    let Some(month) = parse_u32(&bytes[5..7]) else {
        return false;
    };
    let Some(day) = parse_u32(&bytes[8..10]) else {
        return false;
    };
    if !(1..=12).contains(&month) {
        return false;
    }
    let max = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(400) || (year.is_multiple_of(4) && !year.is_multiple_of(100)) => {
            29
        }
        2 => 28,
        _ => 0,
    };
    (1..=max).contains(&day)
}

fn parse_u32(bytes: &[u8]) -> Option<u32> {
    let mut value = 0u32;
    for byte in bytes {
        let digit = byte.checked_sub(b'0')?;
        if digit > 9 {
            return None;
        }
        value = value * 10 + u32::from(digit);
    }
    Some(value)
}

fn parse_range(text: &str) -> Result<Value, Value> {
    let (left, right) = text
        .split_once("..")
        .or_else(|| text.split_once('–'))
        .or_else(|| text.split_once('-'))
        .ok_or_else(|| err("invalid_range", "Not a range"))?;
    let (min, unit_left) = parse_number_text(left.trim())?;
    let (max, unit_right) = parse_number_text(right.trim())?;
    let mut object = Map::new();
    object.insert("min".into(), Value::String(min));
    object.insert("max".into(), Value::String(max));
    if let Some(unit) = unit_left.or(unit_right) {
        object.insert("unit".into(), Value::String(unit));
    }
    Ok(Value::Object(object))
}

fn match_option(
    defs: &DefinitionSet,
    options: &[braian_model::ValueOption],
    list: Option<&str>,
    text: &str,
) -> Result<String, Value> {
    let shared = list.and_then(|key| defs.value_lists.iter().find(|item| item.key == key));
    let inline = options;
    let needle = text.trim().to_lowercase();
    let find = |code: &str, labels: &BTreeMap<LanguageTag, String>| -> bool {
        code.eq_ignore_ascii_case(text.trim())
            || labels.values().any(|label| label.to_lowercase() == needle)
    };
    if let Some(list) = shared {
        for option in &list.options {
            if find(&option.code, &option.labels) {
                return Ok(option.code.clone());
            }
        }
    }
    for option in inline {
        if find(&option.code, &option.labels) {
            return Ok(option.code.clone());
        }
    }
    Err(err("value_not_in_list", "That value is not in the list."))
}

fn diff_patch(
    model: &CompiledModel,
    current: &BTreeMap<AttributeId, Value>,
    next: &BTreeMap<AttributeId, Value>,
    groups: &BTreeMap<braian_model::StructureId, braian_model::GroupId>,
    current_groups: &BTreeMap<braian_model::StructureId, braian_model::GroupId>,
) -> ObjectPatch {
    let mut attributes = BTreeMap::new();
    for (id, value) in next {
        let attribute = model.attribute(*id);
        let changed = current.get(id) != Some(value);
        if !changed {
            continue;
        }
        if attribute.is_some_and(|attribute| attribute.language_dependent) {
            if let Some(map) = value.as_object() {
                let mut languages = BTreeMap::new();
                for (language, inner) in map {
                    let Ok(tag) = LanguageTag::parse(language) else {
                        continue;
                    };
                    let same = current
                        .get(id)
                        .and_then(|existing| existing.get(language))
                        .is_some_and(|existing| existing == inner);
                    if !same {
                        languages.insert(tag, Some(inner.clone()));
                    }
                }
                if !languages.is_empty() {
                    attributes.insert(*id, AttributePatch::Languages { values: languages });
                }
            }
        } else {
            attributes.insert(
                *id,
                AttributePatch::Replace {
                    value: value.clone(),
                },
            );
        }
    }
    let mut placed = BTreeMap::new();
    for (structure, group) in groups {
        if current_groups.get(structure) != Some(group) {
            placed.insert(*structure, Some(*group));
        }
    }
    ObjectPatch {
        values: ValuePatch { attributes },
        groups: placed,
    }
}

fn paint_changes(
    cells: &mut Map<String, Value>,
    model: &CompiledModel,
    columns: &[MappedColumn],
    targets: &[Target],
    current: &BTreeMap<AttributeId, Value>,
    next: &BTreeMap<AttributeId, Value>,
) {
    for column in columns {
        let Some(target) = find_target(targets, &column.target) else {
            continue;
        };
        let Some(key) = target.attribute_key.as_deref() else {
            continue;
        };
        let Some(attribute) = model.attribute_by_key(key) else {
            continue;
        };
        let Some(entry) = cells.get_mut(&column.index.to_string()) else {
            continue;
        };
        let before = current.get(&attribute.id);
        if entry.get("state").and_then(Value::as_str) == Some("error") {
            if let Some(before) = before
                && let Some(object) = entry.as_object_mut()
            {
                object.insert(
                    "before".into(),
                    display_value(before, target.language.as_deref()),
                );
            }
            continue;
        }
        let after = next.get(&attribute.id);
        if before == after {
            if let Some(object) = entry.as_object_mut() {
                object.insert("state".into(), Value::String("same".into()));
            }
            continue;
        }
        if let Some(object) = entry.as_object_mut() {
            object.insert("state".into(), Value::String("changed".into()));
            if let Some(before) = before {
                object.insert(
                    "before".into(),
                    display_value(before, target.language.as_deref()),
                );
            }
        }
    }
}

fn display_value(value: &Value, language: Option<&str>) -> Value {
    if let Some(language) = language
        && let Some(text) = value.get(language)
    {
        return text.clone();
    }
    value.clone()
}

fn attach_validation(
    cells: &mut Map<String, Value>,
    model: &CompiledModel,
    columns: &[MappedColumn],
    targets: &[Target],
    errors: &[braian_model::ValidationError],
) {
    for error in errors {
        let Some(attribute_id) = error.attribute else {
            continue;
        };
        let Some(attribute) = model.attribute(attribute_id) else {
            continue;
        };
        let column = columns.iter().find(|column| {
            let Some(target) = find_target(targets, &column.target) else {
                return false;
            };
            if target.attribute_key.as_deref() != Some(attribute.key.as_str()) {
                return false;
            }
            match (
                error.language.as_ref().map(|tag| tag.as_str()),
                target.language.as_deref(),
            ) {
                (Some(left), Some(right)) => left == right,
                (None, _) => !target.id.contains('@'),
                (Some(_), None) => false,
            }
        });
        let Some(column) = column else { continue };
        if let Some(entry) = cells
            .get_mut(&column.index.to_string())
            .and_then(Value::as_object_mut)
        {
            entry.insert("state".into(), Value::String("error".into()));
            entry.insert("error".into(), error_json(error));
        }
    }
}

fn error_json(error: &braian_model::ValidationError) -> Value {
    json!({ "code": error.code, "message": friendly_validation(error) })
}

fn friendly_validation(error: &braian_model::ValidationError) -> String {
    match error.code {
        ErrorCode::InvalidDate => "Not a date".to_owned(),
        ErrorCode::InvalidDecimal | ErrorCode::NotInteger => "Not a number".to_owned(),
        ErrorCode::ValueNotInList => "That value is not in the list.".to_owned(),
        ErrorCode::TextTooLong => "That text is too long.".to_owned(),
        _ => error.message.clone(),
    }
}

fn object_key_value(model: &CompiledModel, type_id: TypeId, object: &Object) -> Option<String> {
    let id = model.key_attribute(type_id)?;
    object
        .values
        .get(&id)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn err(code: &str, message: &str) -> Value {
    json!({ "code": code, "message": message })
}

pub fn seen_key(seen: &mut HashMap<String, u32>, key: &str, row_no: u32) -> Option<u32> {
    if let Some(first) = seen.get(key) {
        Some(*first)
    } else {
        seen.insert(key.to_owned(), row_no);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defs() -> DefinitionSet {
        serde_json::from_str(include_str!("../../../../testdata/rules/definitions.json")).unwrap()
    }

    fn model() -> (CompiledModel, DefinitionSet) {
        let defs = defs();
        let model = CompiledModel::compile(&defs).unwrap();
        (model, defs)
    }

    #[test]
    fn number_locales_dates_booleans_and_lists() {
        assert_eq!(parse_number_text("1,234").unwrap().0, "1234");
        assert_eq!(parse_number_text("1,23").unwrap().0, "1.23");
        assert_eq!(parse_number_text("1234.5").unwrap().0, "1234.5");
        assert_eq!(parse_number_text("1.234,5").unwrap().0, "1234.5");
        assert_eq!(
            parse_number_text("5 kg").unwrap(),
            ("5".into(), Some("kg".into()))
        );
        let (model, defs) = model();
        let color = model.attribute_by_key("color").unwrap();
        let parsed = parse_cell(&defs, color, None, "Röd", &mut |_type, _key| None).unwrap();
        assert_eq!(parsed, Value::String("red".into()));
        let active = model.attribute_by_key("active").unwrap();
        assert_eq!(
            parse_cell(&defs, active, None, "ja", &mut |_type, _key| None).unwrap(),
            Value::Bool(true)
        );
        let made = model.attribute_by_key("made").unwrap();
        assert_eq!(
            parse_cell(&defs, made, None, "31/12/2024", &mut |_type, _key| None).unwrap(),
            Value::String("2024-12-31".into())
        );
    }

    #[test]
    fn classifies_new_changed_unchanged_delete_and_duplicates() {
        let (model, defs) = model();
        let targets = super::super::mapping::targets_for(&defs, "item");
        let columns = vec![
            MappedColumn {
                index: 0,
                target: "type:sku".into(),
            },
            MappedColumn {
                index: 1,
                target: "type:name".into(),
            },
            MappedColumn {
                index: 2,
                target: "action".into(),
            },
        ];
        let mut resolve = |_type: &str, _key: &str| None;
        let created = classify_row(
            &model,
            &defs,
            &targets,
            "item",
            &columns,
            Some(0),
            2,
            &[Some("A-1".into()), Some("Stol".into()), None],
            None,
            None,
            &mut resolve,
        );
        assert_eq!(created.outcome.status, "new");
        let mut seen = HashMap::new();
        assert_eq!(seen_key(&mut seen, "A-1", 2), None);
        assert_eq!(seen_key(&mut seen, "A-1", 3), Some(2));
        let duplicate = classify_row(
            &model,
            &defs,
            &targets,
            "item",
            &columns,
            Some(0),
            3,
            &[Some("A-1".into()), Some("Annan".into()), None],
            None,
            Some(2),
            &mut resolve,
        );
        assert_eq!(duplicate.outcome.errors[0]["code"], "duplicate_id");
        let existing = StoredObject {
            object: serde_json::from_value(created.outcome.patch.clone().unwrap()).unwrap(),
            key: "A-1".into(),
            version: 4,
            deleted_at: None,
        };
        let same = classify_row(
            &model,
            &defs,
            &targets,
            "item",
            &columns,
            Some(0),
            4,
            &[Some("A-1".into()), Some("Stol".into()), None],
            Some(&existing),
            None,
            &mut resolve,
        );
        assert_eq!(same.outcome.status, "unchanged");
        let changed = classify_row(
            &model,
            &defs,
            &targets,
            "item",
            &columns,
            Some(0),
            5,
            &[Some("A-1".into()), Some("Bord".into()), Some("".into())],
            Some(&existing),
            None,
            &mut resolve,
        );
        assert_eq!(changed.outcome.status, "changed");
        assert_eq!(changed.outcome.cells["1"]["state"], "changed");
        let empty = classify_row(
            &model,
            &defs,
            &targets,
            "item",
            &columns,
            Some(0),
            6,
            &[Some("A-1".into()), None, None],
            Some(&existing),
            None,
            &mut resolve,
        );
        assert_eq!(empty.outcome.status, "unchanged");
        let deleted = classify_row(
            &model,
            &defs,
            &targets,
            "item",
            &columns,
            Some(0),
            7,
            &[Some("A-1".into()), None, Some("delete".into())],
            Some(&existing),
            None,
            &mut resolve,
        );
        assert_eq!(deleted.outcome.status, "delete");
        let missing = classify_row(
            &model,
            &defs,
            &targets,
            "item",
            &columns,
            Some(0),
            8,
            &[Some("B-9".into()), None, Some("delete".into())],
            None,
            None,
            &mut resolve,
        );
        assert_eq!(missing.outcome.errors[0]["code"], "delete_not_found");
        assert_ne!(deleted.outcome.cells["0"]["state"], "new");
        assert_eq!(deleted.outcome.cells["0"]["state"], "same");
    }

    #[test]
    fn invalid_date_is_on_the_date_cell_and_existing_rows_keep_before() {
        let (model, defs) = model();
        let targets = super::super::mapping::targets_for(&defs, "item");
        let columns = vec![
            MappedColumn {
                index: 0,
                target: "type:sku".into(),
            },
            MappedColumn {
                index: 1,
                target: "type:name".into(),
            },
            MappedColumn {
                index: 2,
                target: "type:made".into(),
            },
            MappedColumn {
                index: 3,
                target: "type:active".into(),
            },
        ];
        let mut resolve = |_type: &str, _key: &str| None;
        let created = classify_row(
            &model,
            &defs,
            &targets,
            "item",
            &columns,
            Some(0),
            2,
            &[
                Some("A-1".into()),
                Some("Old".into()),
                Some("2024-01-02".into()),
                Some("ja".into()),
            ],
            None,
            None,
            &mut resolve,
        );
        assert_eq!(created.outcome.status, "new");
        let existing = StoredObject {
            object: serde_json::from_value(created.outcome.patch.clone().unwrap()).unwrap(),
            key: "A-1".into(),
            version: 2,
            deleted_at: None,
        };
        let dated = classify_row(
            &model,
            &defs,
            &targets,
            "item",
            &columns,
            Some(0),
            3,
            &[
                Some("A-1".into()),
                Some("New".into()),
                Some("2025-13-45".into()),
                Some("ja".into()),
            ],
            Some(&existing),
            None,
            &mut resolve,
        );
        assert_eq!(dated.outcome.status, "failed");
        assert_eq!(dated.outcome.cells["2"]["state"], "error");
        assert_eq!(dated.outcome.cells["2"]["error"]["code"], "invalid_date");
        assert_eq!(dated.outcome.cells["2"]["error"]["message"], "Not a date");
        assert_ne!(dated.outcome.cells["0"]["state"], "error");
        assert_eq!(dated.outcome.cells["0"]["state"], "same");
        assert_ne!(dated.outcome.cells["0"]["state"], "new");
        assert_eq!(dated.outcome.cells["1"]["state"], "changed");
        assert!(dated.outcome.cells["1"].get("before").is_some());
        let flagged = classify_row(
            &model,
            &defs,
            &targets,
            "item",
            &columns,
            Some(0),
            4,
            &[
                Some("A-1".into()),
                Some("New".into()),
                Some("2024-01-02".into()),
                Some("kanske".into()),
            ],
            Some(&existing),
            None,
            &mut resolve,
        );
        assert_eq!(flagged.outcome.status, "failed");
        assert_eq!(
            flagged.outcome.cells["3"]["error"]["code"],
            "invalid_boolean"
        );
        assert_eq!(flagged.outcome.cells["0"]["state"], "same");
        assert_ne!(flagged.outcome.cells["0"]["state"], "new");
        assert_eq!(flagged.outcome.cells["1"]["state"], "changed");
        assert!(flagged.outcome.cells["1"].get("before").is_some());
    }
}
