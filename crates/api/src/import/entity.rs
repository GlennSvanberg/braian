//! Import of data model entities. A sheet with one entity per file, like an object import,
//! but the rows become model changes instead of objects. Accepting writes them as one revision.
//!
//! Built so far: attributes. Types, structures, groups, classes, value lists, unit families
//! and languages follow the same shape: a token, targets, and a row classifier.

use braian_model::{Attribute, AttributeKind, DefinitionSet};
use serde_json::{Value, json};

use super::diff::{MappedColumn, RowOutcome};
use super::mapping::Target;

pub const ATTRIBUTE: &str = "model:attribute";

pub fn is_model(token: &str) -> bool {
    token.starts_with("model:")
}

/// The entities offered next to the object types: (token, label).
pub fn options() -> Vec<(&'static str, &'static str)> {
    vec![(ATTRIBUTE, "Data model: Attributes")]
}

fn target(id: &str, label: &str, kind: &str, key: bool) -> Target {
    Target {
        id: format!("{ATTRIBUTE}:{id}"),
        label: label.to_owned(),
        kind: kind.to_owned(),
        key_attribute: key,
        source: "Data model".to_owned(),
        attribute_key: None,
        language: None,
        structure_key: None,
        group_key: None,
        unit: None,
        attribute_id: None,
        structure_id: None,
        group_id: None,
    }
}

pub fn targets(token: &str) -> Vec<Target> {
    if token != ATTRIBUTE {
        return Vec::new();
    }
    vec![
        target("key", "Key", "text", true),
        target("label", "Label", "text", false),
        target("kind", "Kind", "text", false),
        target("unit", "Unit", "text", false),
        target("multiple", "Multiple values", "boolean", false),
        target("languages", "Language dependent", "boolean", false),
        target("required", "Required", "boolean", false),
        target("searchable", "Searchable", "boolean", false),
        target("filterable", "Filterable", "boolean", false),
        target("sortable", "Sortable", "boolean", false),
    ]
}

const KINDS: &[&str] = &[
    "text",
    "number",
    "boolean",
    "date",
    "value_list",
    "range",
    "file",
];

fn kind_name(kind: &AttributeKind) -> &'static str {
    match kind {
        AttributeKind::Text { .. } => "text",
        AttributeKind::Number { .. } => "number",
        AttributeKind::Boolean => "boolean",
        AttributeKind::Date => "date",
        AttributeKind::ValueList { .. } => "value_list",
        AttributeKind::Range { .. } => "range",
        AttributeKind::FileRef { .. } => "file",
        AttributeKind::ObjectRef { .. } => "object_ref",
    }
}

fn parse_flag(text: &str) -> Option<bool> {
    match text.trim().to_lowercase().as_str() {
        "true" | "yes" | "y" | "1" | "ja" | "x" => Some(true),
        "false" | "no" | "n" | "0" | "nej" | "" => Some(false),
        _ => None,
    }
}

fn label_of(attribute: &Attribute) -> String {
    attribute
        .labels
        .iter()
        .find(|(tag, _)| tag.as_str() == "en")
        .or_else(|| attribute.labels.iter().next())
        .map(|(_, text)| text.clone())
        .unwrap_or_default()
}

fn field(
    columns: &[MappedColumn],
    cells: &[Option<String>],
    name: &str,
) -> Option<(usize, String)> {
    let id = format!("{ATTRIBUTE}:{name}");
    let column = columns.iter().find(|column| column.target == id)?;
    let text = cells.get(column.index).and_then(|cell| cell.clone())?;
    let text = text.trim().to_owned();
    (!text.is_empty()).then_some((column.index, text))
}

/// Classifies one attribute row against the current definitions.
pub fn classify_attribute(
    defs: &DefinitionSet,
    columns: &[MappedColumn],
    id_column: Option<usize>,
    cells: &[Option<String>],
) -> RowOutcome {
    let mut view = serde_json::Map::new();
    for column in columns {
        let value = cells
            .get(column.index)
            .and_then(|cell| cell.clone())
            .unwrap_or_default();
        view.insert(
            column.index.to_string(),
            json!({ "value": value, "state": "same" }),
        );
    }
    let mut errors: Vec<Value> = Vec::new();
    let mut fail = |view: &mut serde_json::Map<String, Value>,
                    index: Option<usize>,
                    code: &str,
                    message: &str| {
        let error = json!({ "code": code, "message": message });
        if let Some(index) = index
            && let Some(cell) = view
                .get_mut(&index.to_string())
                .and_then(Value::as_object_mut)
        {
            cell.insert("state".into(), Value::String("error".into()));
            cell.insert("error".into(), error.clone());
        }
        errors.push(error);
    };
    let key_cell = id_column
        .and_then(|index| {
            cells
                .get(index)
                .and_then(|cell| cell.clone())
                .map(|text| (index, text))
        })
        .or_else(|| field(columns, cells, "key"))
        .map(|(index, text)| (index, text.trim().to_owned()))
        .filter(|(_, text)| !text.is_empty());
    let Some((key_index, key)) = key_cell else {
        fail(
            &mut view,
            None,
            "missing_key",
            "The key is empty, so the row cannot be matched.",
        );
        return outcome("failed", None, view, None, errors);
    };
    let valid_key = key.chars().all(|character| {
        character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
    }) && key
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_lowercase());
    if !valid_key {
        fail(
            &mut view,
            Some(key_index),
            "invalid_key",
            "A key uses lowercase letters, digits and underscores, and starts with a letter.",
        );
    }
    let label = field(columns, cells, "label");
    let kind = field(columns, cells, "kind");
    let unit = field(columns, cells, "unit");
    if let Some((index, text)) = &kind
        && !KINDS.contains(&text.to_lowercase().as_str())
    {
        fail(
            &mut view,
            Some(*index),
            "invalid_kind",
            &format!("The kind must be one of {}.", KINDS.join(", ")),
        );
    }
    let mut flags: Vec<(&str, Option<bool>)> = Vec::new();
    for name in [
        "multiple",
        "languages",
        "required",
        "searchable",
        "filterable",
        "sortable",
    ] {
        match field(columns, cells, name) {
            Some((index, text)) => match parse_flag(&text) {
                Some(value) => flags.push((name, Some(value))),
                None => {
                    fail(&mut view, Some(index), "invalid_flag", "Use yes or no.");
                    flags.push((name, None));
                }
            },
            None => flags.push((name, None)),
        }
    }
    if !errors.is_empty() {
        return outcome("failed", Some(key), view, None, errors);
    }
    let flag = |name: &str| {
        flags
            .iter()
            .find(|(item, _)| *item == name)
            .and_then(|(_, value)| *value)
    };
    let existing = defs
        .attributes
        .iter()
        .find(|attribute| attribute.key == key);
    let Some(existing) = existing else {
        let mut change = json!({
            "op": "create_attribute",
            "key": key,
            "label": label.as_ref().map(|(_, text)| text.clone()).unwrap_or_else(|| key.clone()),
            "kind": kind.as_ref().map(|(_, text)| text.to_lowercase()).unwrap_or_else(|| "text".into()),
            "multiple": flag("multiple").unwrap_or(false),
            "languages": flag("languages").unwrap_or(false),
            "required": flag("required").unwrap_or(false),
            "searchable": flag("searchable").unwrap_or(false),
            "filterable": flag("filterable").unwrap_or(true),
            "sortable": flag("sortable").unwrap_or(false),
        });
        if let Some((_, unit)) = &unit {
            change["unit"] = Value::String(unit.clone());
        }
        for column in columns {
            if let Some(cell) = view
                .get_mut(&column.index.to_string())
                .and_then(Value::as_object_mut)
            {
                cell.insert("state".into(), Value::String("new".into()));
            }
        }
        return outcome("new", Some(key), view, Some(change), Vec::new());
    };
    let mut change = json!({ "op": "update_attribute", "key": key });
    let mut changed = false;
    let paint = |view: &mut serde_json::Map<String, Value>, name: &str, before: String| {
        let id = format!("{ATTRIBUTE}:{name}");
        if let Some(column) = columns.iter().find(|column| column.target == id)
            && let Some(cell) = view
                .get_mut(&column.index.to_string())
                .and_then(Value::as_object_mut)
        {
            cell.insert("state".into(), Value::String("changed".into()));
            cell.insert("before".into(), Value::String(before));
        }
    };
    if let Some((_, text)) = &label
        && *text != label_of(existing)
    {
        change["label"] = Value::String(text.clone());
        paint(&mut view, "label", label_of(existing));
        changed = true;
    }
    if let Some((_, text)) = &kind {
        let wanted = text.to_lowercase();
        if wanted != kind_name(&existing.kind) {
            change["kind"] = Value::String(wanted);
            paint(&mut view, "kind", kind_name(&existing.kind).to_owned());
            changed = true;
        }
    }
    if let Some((_, text)) = &unit {
        change["unit"] = Value::String(text.clone());
        changed = true;
    }
    let current = [
        ("multiple", existing.multiple),
        ("languages", existing.language_dependent),
        ("required", existing.required),
        ("searchable", existing.index.searchable),
        ("filterable", existing.index.filterable),
        ("sortable", existing.index.sortable),
    ];
    for (name, before) in current {
        if let Some(value) = flag(name)
            && value != before
        {
            change[name] = Value::Bool(value);
            paint(
                &mut view,
                name,
                if before { "yes" } else { "no" }.to_owned(),
            );
            changed = true;
        }
    }
    if changed {
        outcome("changed", Some(key), view, Some(change), Vec::new())
    } else {
        outcome("unchanged", Some(key), view, None, Vec::new())
    }
}

fn outcome(
    status: &str,
    key: Option<String>,
    view: serde_json::Map<String, Value>,
    patch: Option<Value>,
    errors: Vec<Value>,
) -> RowOutcome {
    RowOutcome {
        status: status.to_owned(),
        object_id: None,
        object_key: key,
        base_version: None,
        cells: Value::Object(view),
        patch,
        errors: Value::Array(errors),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defs() -> DefinitionSet {
        DefinitionSet {
            model_revision: 1,
            languages: Vec::new(),
            unit_families: Vec::new(),
            value_lists: Vec::new(),
            attributes: Vec::new(),
            types: Vec::new(),
            structures: Vec::new(),
        }
    }

    fn columns() -> Vec<MappedColumn> {
        ["key", "label", "kind", "required"]
            .iter()
            .enumerate()
            .map(|(index, name)| MappedColumn {
                index,
                target: format!("{ATTRIBUTE}:{name}"),
                clear_empty: false,
                skip_invalid: false,
            })
            .collect()
    }

    fn row(values: &[&str]) -> Vec<Option<String>> {
        values
            .iter()
            .map(|value| Some((*value).to_owned()))
            .collect()
    }

    #[test]
    fn a_new_key_becomes_a_create_step() {
        let result = classify_attribute(
            &defs(),
            &columns(),
            Some(0),
            &row(&["material", "Material", "text", "yes"]),
        );
        assert_eq!(result.status, "new");
        let patch = result.patch.expect("patch");
        assert_eq!(patch["op"], "create_attribute");
        assert_eq!(patch["required"], true);
        let change: braian_model::ModelChange =
            serde_json::from_value(patch).expect("model change");
        assert!(matches!(
            change,
            braian_model::ModelChange::CreateAttribute { .. }
        ));
    }

    #[test]
    fn bad_kind_and_key_fail_with_cell_errors() {
        let result = classify_attribute(
            &defs(),
            &columns(),
            Some(0),
            &row(&["Bad Key", "X", "colour", ""]),
        );
        assert_eq!(result.status, "failed");
        assert_eq!(result.errors.as_array().map(Vec::len), Some(2));
        let empty = classify_attribute(&defs(), &columns(), Some(0), &row(&["", "X", "text", ""]));
        assert_eq!(empty.status, "failed");
    }

    #[test]
    fn targets_only_exist_for_known_entities() {
        assert!(
            targets("model:attribute")
                .iter()
                .any(|item| item.key_attribute)
        );
        assert!(targets("model:other").is_empty());
    }
}
