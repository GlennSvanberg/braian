//! Pathless columns the object table may show.
//!
//! Attribute paths come from [`crate::list_attribute_paths`]. These keys are the
//! only other values a column-prefs document may store. The API exposes this
//! list on `/v1/types/{id}/attribute-paths` so the web app never hard-codes it.

use serde_json::{Map, Value, json};

/// One pathless column the object table can display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemViewColumn {
    pub key: &'static str,
    pub label: &'static str,
    /// Index among the default ObjectTable columns
    /// (`number`, `name`, `type`, `status`, `version`).
    pub default_index: usize,
}

/// Sole allowlist of pathless view columns. Checked in tree for ObjectTable:
/// only `type` (search `type_key`) and `version` lack an attribute path.
/// `number`, `name`, `status`, `created_at`, `updated_at`, and `updated_by`
/// are attribute paths. `id` is not a table column today.
pub const SYSTEM_VIEW_COLUMNS: &[SystemViewColumn] = &[
    SystemViewColumn {
        key: "type",
        label: "Type",
        default_index: 2,
    },
    SystemViewColumn {
        key: "version",
        label: "Version",
        default_index: 4,
    },
];

pub fn is_system_view_column(key: &str) -> bool {
    SYSTEM_VIEW_COLUMNS.iter().any(|column| column.key == key)
}

/// Rank of a key in the default ObjectTable layout, when known.
fn default_rank(key: &str) -> Option<usize> {
    match key {
        "number" | "key" => Some(0),
        "name" => Some(1),
        "type" => Some(2),
        "status" => Some(3),
        "version" => Some(4),
        _ => None,
    }
}

/// JSON rows for the attribute-paths response (`system_columns`).
pub fn system_view_columns_json() -> Value {
    Value::Array(
        SYSTEM_VIEW_COLUMNS
            .iter()
            .map(|column| {
                json!({
                    "key": column.key,
                    "label": column.label,
                    "default_index": column.default_index,
                })
            })
            .collect(),
    )
}

/// Insert missing system columns as visible entries at their default positions.
///
/// Used on GET so prefs saved before this allowlist still show `type` and
/// `version`. Existing entries (including hidden ones) are left alone.
pub fn ensure_system_view_columns(document: &Value) -> Value {
    let Some(object) = document.as_object() else {
        return document.clone();
    };
    let Some(columns) = object.get("columns").and_then(Value::as_array) else {
        return document.clone();
    };
    let mut next = columns.clone();
    for system in SYSTEM_VIEW_COLUMNS {
        if next.iter().any(|column| {
            column
                .get("path")
                .and_then(Value::as_str)
                .is_some_and(|path| path == system.key)
        }) {
            continue;
        }
        let insert_at = next
            .iter()
            .enumerate()
            .filter_map(|(index, column)| {
                let path = column.get("path").and_then(Value::as_str)?;
                let rank = default_rank(path)?;
                (rank < system.default_index).then_some(index + 1)
            })
            .max()
            // No ranked neighbors: park at the absolute default index (clamped).
            .unwrap_or_else(|| system.default_index.min(next.len()));
        let insert_at = insert_at.min(next.len());
        next.insert(
            insert_at,
            json!({
                "path": system.key,
                "visible": true,
            }),
        );
    }
    let mut out = Map::new();
    for (key, value) in object {
        if key == "columns" {
            out.insert(key.clone(), Value::Array(next.clone()));
        } else {
            out.insert(key.clone(), value.clone());
        }
    }
    if !out.contains_key("columns") {
        out.insert("columns".into(), Value::Array(next));
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_inserts_missing_system_columns_at_default_ranks() {
        let document = json!({
            "version": 1,
            "columns": [
                { "path": "number", "visible": true },
                { "path": "name", "visible": true },
                { "path": "status", "visible": false }
            ]
        });
        let ensured = ensure_system_view_columns(&document);
        let paths: Vec<_> = ensured["columns"]
            .as_array()
            .unwrap()
            .iter()
            .map(|column| column["path"].as_str().unwrap())
            .collect();
        assert_eq!(paths, ["number", "name", "type", "status", "version"]);
        assert_eq!(ensured["columns"][2]["visible"], true);
        assert_eq!(ensured["columns"][4]["visible"], true);
    }

    #[test]
    fn ensure_keeps_hidden_system_columns() {
        let document = json!({
            "version": 1,
            "columns": [
                { "path": "type", "visible": false },
                { "path": "name", "visible": true }
            ]
        });
        let ensured = ensure_system_view_columns(&document);
        assert_eq!(ensured["columns"][0]["path"], "type");
        assert_eq!(ensured["columns"][0]["visible"], false);
        assert!(
            ensured["columns"]
                .as_array()
                .unwrap()
                .iter()
                .any(|column| column["path"] == "version" && column["visible"] == true)
        );
    }

    #[test]
    fn ensure_parks_at_default_index_when_no_ranked_neighbors() {
        let document = json!({
            "version": 1,
            "columns": [
                { "path": "sku", "visible": true },
                { "path": "weight", "visible": true }
            ]
        });
        let ensured = ensure_system_view_columns(&document);
        let paths: Vec<_> = ensured["columns"]
            .as_array()
            .unwrap()
            .iter()
            .map(|column| column["path"].as_str().unwrap())
            .collect();
        assert_eq!(paths, ["sku", "weight", "type", "version"]);
    }
}
