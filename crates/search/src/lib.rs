//! Search interface. Typesense is one implementation; callers do not talk to it.
//!
//! Spike 2 stores one document per object and one field per language. Numbers
//! also get the value in the family's base unit, as a float used only for
//! sorting and range filters. The exact decimal stays in Postgres.

use std::collections::BTreeMap;

use braian_model::{AttributeKind, DefinitionSet, Object};
use braian_rules::CompiledModel;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub const ALIAS: &str = "braian_objects";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("typesense: {0}")]
    Typesense(String),
    #[error("search json: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Default)]
pub struct SearchQuery {
    pub text: String,
    pub language: String,
    pub filter_by: String,
    pub sort_by: String,
    pub group_by: String,
    pub page: u32,
    pub per_page: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub id: String,
    pub document: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchGroup {
    pub key: String,
    pub found: u32,
    pub hits: Vec<SearchHit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchPage {
    pub found: u32,
    pub hits: Vec<SearchHit>,
    pub groups: Vec<SearchGroup>,
}

#[derive(Clone)]
pub struct Typesense {
    client: reqwest::Client,
    base: String,
    api_key: String,
    alias: String,
}

impl Typesense {
    pub fn new(base: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            base: base.into().trim_end_matches('/').to_owned(),
            api_key: api_key.into(),
            alias: ALIAS.to_owned(),
        }
    }

    /// Tests use a private alias so they do not share one collection.
    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.alias = alias.into();
        self
    }

    pub fn alias(&self) -> &str {
        &self.alias
    }

    pub async fn health(&self) -> Result<(), Error> {
        let response = self
            .client
            .get(format!("{}/health", self.base))
            .header("X-TYPESENSE-API-KEY", &self.api_key)
            .send()
            .await
            .map_err(|error| Error::Typesense(error.to_string()))?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(Error::Typesense(format!("health {}", response.status())))
        }
    }

    pub async fn upsert_alias(&self, collection: &str) -> Result<(), Error> {
        self.send(
            reqwest::Method::PUT,
            &format!("/aliases/{}", self.alias),
            Some(json!({ "collection_name": collection })),
        )
        .await
        .map(|_| ())
    }

    pub async fn alias_target(&self) -> Result<Option<String>, Error> {
        let response = self
            .client
            .get(format!("{}/aliases/{}", self.base, self.alias))
            .header("X-TYPESENSE-API-KEY", &self.api_key)
            .send()
            .await
            .map_err(|error| Error::Typesense(error.to_string()))?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let body: Value = response
            .json()
            .await
            .map_err(|error| Error::Typesense(error.to_string()))?;
        Ok(body
            .get("collection_name")
            .and_then(Value::as_str)
            .map(str::to_owned))
    }

    pub async fn create_collection(&self, name: &str, defs: &DefinitionSet) -> Result<(), Error> {
        let mut fields = vec![
            json!({"name": "key", "type": "string"}),
            json!({"name": "type_key", "type": "string", "facet": true}),
            json!({"name": "seq", "type": "int64"}),
            json!({"name": "version", "type": "int64"}),
            json!({"name": "deleted", "type": "bool", "facet": true}),
            json!({"name": "groups", "type": "string[]", "facet": true}),
        ];
        for language in &defs.languages {
            fields.push(json!({
                "name": format!("search_{}", field_language(language.as_str())),
                "type": "string",
                "optional": true,
            }));
        }
        for attribute in &defs.attributes {
            if !indexed(attribute) {
                continue;
            }
            // A searchable-only attribute is matched through `search_{lang}`. Its own
            // field is kept for display but not indexed, so Typesense does not build
            // a token index per attribute and language.
            let own_index = attribute.index.filterable || attribute.index.sortable;
            for name in field_names(attribute, defs) {
                fields.push(json!({
                    "name": name,
                    "type": field_type(attribute),
                    "optional": true,
                    "index": own_index,
                    "facet": attribute.index.filterable,
                    "sort": attribute.index.sortable && !attribute.multiple,
                }));
            }
        }
        self.send(
            reqwest::Method::POST,
            "/collections",
            Some(json!({ "name": name, "fields": fields })),
        )
        .await
        .map(|_| ())
    }

    pub async fn delete_document(&self, id: &str) -> Result<(), Error> {
        let response = self
            .client
            .delete(format!(
                "{}/collections/{}/documents/{id}",
                self.base, self.alias
            ))
            .header("X-TYPESENSE-API-KEY", &self.api_key)
            .send()
            .await
            .map_err(|error| Error::Typesense(error.to_string()))?;
        if response.status().is_success() || response.status() == reqwest::StatusCode::NOT_FOUND {
            Ok(())
        } else {
            Err(Error::Typesense(format!(
                "delete {id}: {}",
                response.status()
            )))
        }
    }

    pub async fn drop_collection(&self, name: &str) -> Result<(), Error> {
        let response = self
            .client
            .delete(format!("{}/collections/{name}", self.base))
            .header("X-TYPESENSE-API-KEY", &self.api_key)
            .send()
            .await
            .map_err(|error| Error::Typesense(error.to_string()))?;
        if response.status().is_success() || response.status() == reqwest::StatusCode::NOT_FOUND {
            Ok(())
        } else {
            Err(Error::Typesense(format!(
                "drop {name}: {}",
                response.status()
            )))
        }
    }

    pub async fn import(&self, collection: &str, docs: &[Value]) -> Result<(), Error> {
        if docs.is_empty() {
            return Ok(());
        }
        let mut body = String::new();
        for doc in docs {
            body.push_str(&serde_json::to_string(doc)?);
            body.push('\n');
        }
        let response = self
            .client
            .post(format!(
                "{}/collections/{collection}/documents/import?action=upsert",
                self.base
            ))
            .header("X-TYPESENSE-API-KEY", &self.api_key)
            .header("Content-Type", "text/plain")
            .body(body)
            .send()
            .await
            .map_err(|error| Error::Typesense(error.to_string()))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|error| Error::Typesense(error.to_string()))?;
        if !status.is_success() || text.contains("\"success\":false") {
            return Err(Error::Typesense(format!("import {status}: {text}")));
        }
        Ok(())
    }

    pub async fn search(&self, query: &SearchQuery) -> Result<SearchPage, Error> {
        let language = if query.language.is_empty() {
            "sv"
        } else {
            query.language.as_str()
        };
        let query_by = format!("key,search_{}", field_language(language));
        let mut params = vec![
            (
                "q",
                if query.text.is_empty() {
                    "*".to_owned()
                } else {
                    query.text.clone()
                },
            ),
            ("query_by", query_by),
            ("page", query.page.max(1).to_string()),
            ("per_page", query.per_page.clamp(1, 250).to_string()),
            (
                "filter_by",
                if query.filter_by.is_empty() {
                    "deleted:false".to_owned()
                } else if query.filter_by.contains("deleted:") {
                    query.filter_by.clone()
                } else {
                    format!("deleted:false && {}", query.filter_by)
                },
            ),
        ];
        if !query.sort_by.is_empty() {
            params.push(("sort_by", query.sort_by.clone()));
        }
        if !query.group_by.is_empty() {
            params.push(("group_by", query.group_by.clone()));
            params.push(("group_limit", "20".to_owned()));
        }
        let response = self
            .client
            .get(format!(
                "{}/collections/{}/documents/search",
                self.base, self.alias
            ))
            .header("X-TYPESENSE-API-KEY", &self.api_key)
            .query(&params)
            .send()
            .await
            .map_err(|error| Error::Typesense(error.to_string()))?;
        let status = response.status();
        let body: Value = response
            .json()
            .await
            .map_err(|error| Error::Typesense(error.to_string()))?;
        if !status.is_success() {
            return Err(Error::Typesense(format!("search {status}: {body}")));
        }
        Ok(parse_page(&body))
    }

    async fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, Error> {
        let mut request = self
            .client
            .request(method, format!("{}{path}", self.base))
            .header("X-TYPESENSE-API-KEY", &self.api_key);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .await
            .map_err(|error| Error::Typesense(error.to_string()))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|error| Error::Typesense(error.to_string()))?;
        if !status.is_success() {
            return Err(Error::Typesense(format!("{status}: {text}")));
        }
        if text.is_empty() {
            return Ok(Value::Null);
        }
        Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }
}

fn parse_page(body: &Value) -> SearchPage {
    let found = body.get("found").and_then(Value::as_u64).unwrap_or(0) as u32;
    let hits = body
        .get("hits")
        .and_then(Value::as_array)
        .map(|hits| hits.iter().filter_map(parse_hit).collect())
        .unwrap_or_default();
    let groups = body
        .get("grouped_hits")
        .and_then(Value::as_array)
        .map(|groups| {
            groups
                .iter()
                .map(|group| SearchGroup {
                    key: group
                        .get("group_key")
                        .map(|key| match key {
                            Value::Array(items) => items
                                .iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join(","),
                            other => other.to_string(),
                        })
                        .unwrap_or_default(),
                    found: group.get("found").and_then(Value::as_u64).unwrap_or(0) as u32,
                    hits: group
                        .get("hits")
                        .and_then(Value::as_array)
                        .map(|hits| hits.iter().filter_map(parse_hit).collect())
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default();
    SearchPage {
        found,
        hits,
        groups,
    }
}

fn parse_hit(value: &Value) -> Option<SearchHit> {
    let document = value.get("document")?.clone();
    let id = document.get("id")?.as_str()?.to_owned();
    Some(SearchHit { id, document })
}

fn indexed(attribute: &braian_model::Attribute) -> bool {
    attribute.index.searchable || attribute.index.filterable || attribute.index.sortable
}

fn field_type(attribute: &braian_model::Attribute) -> &'static str {
    let numeric = matches!(
        attribute.kind,
        AttributeKind::Number { .. } | AttributeKind::Range { .. }
    );
    if attribute.multiple {
        if numeric { "float[]" } else { "string[]" }
    } else if numeric {
        "float"
    } else {
        "string"
    }
}

fn field_names(attribute: &braian_model::Attribute, defs: &DefinitionSet) -> Vec<String> {
    let base = format!("a_{}", attribute.key);
    if attribute.language_dependent {
        defs.languages
            .iter()
            .map(|language| format!("{base}__{}", field_language(language.as_str())))
            .collect()
    } else {
        vec![base]
    }
}

pub fn field_language(tag: &str) -> String {
    tag.replace('-', "_")
}

/// One search document for the current object. Deleted objects are marked, not removed,
/// so a stale event cannot resurrect them; the caller may also delete the document.
#[allow(clippy::too_many_arguments)]
pub fn document(
    model: &CompiledModel,
    defs: &DefinitionSet,
    object: &Object,
    key: &str,
    type_key: &str,
    version: u64,
    seq: i64,
    deleted: bool,
) -> Value {
    let mut doc = Map::new();
    doc.insert("id".into(), Value::String(object.id.to_string()));
    doc.insert("key".into(), Value::String(key.to_owned()));
    doc.insert("type_key".into(), Value::String(type_key.to_owned()));
    doc.insert("seq".into(), json!(seq));
    doc.insert("version".into(), json!(version));
    doc.insert("deleted".into(), Value::Bool(deleted));
    let groups: Vec<Value> = object
        .groups
        .iter()
        .filter_map(|(structure, group)| {
            let structure = defs.structures.iter().find(|item| item.id == *structure)?;
            let group = structure.groups.iter().find(|item| item.id == *group)?;
            Some(Value::String(format!("{}:{}", structure.key, group.key)))
        })
        .collect();
    doc.insert("groups".into(), Value::Array(groups));

    let mut blobs: BTreeMap<String, String> = BTreeMap::new();
    for attribute in &defs.attributes {
        if !indexed(attribute) {
            continue;
        }
        let Some(value) = object.values.get(&attribute.id) else {
            continue;
        };
        if attribute.language_dependent {
            let Some(map) = value.as_object() else {
                continue;
            };
            for language in &defs.languages {
                let Some(inner) = map.get(language.as_str()) else {
                    continue;
                };
                let field = format!("a_{}__{}", attribute.key, field_language(language.as_str()));
                if let Some(indexed) = index_value(model, attribute, inner) {
                    if attribute.index.searchable {
                        push_blob(&mut blobs, field_language(language.as_str()), &indexed);
                    }
                    doc.insert(field, indexed_to_json(indexed));
                }
            }
        } else if let Some(indexed) = index_value(model, attribute, value) {
            if attribute.index.searchable {
                for language in &defs.languages {
                    push_blob(&mut blobs, field_language(language.as_str()), &indexed);
                }
                if defs.languages.is_empty() {
                    push_blob(&mut blobs, "sv".into(), &indexed);
                }
            }
            doc.insert(format!("a_{}", attribute.key), indexed_to_json(indexed));
        }
    }
    for (language, text) in blobs {
        doc.insert(format!("search_{language}"), Value::String(text));
    }
    Value::Object(doc)
}

enum Indexed {
    Text(String),
    Number(f64),
    Texts(Vec<String>),
    Numbers(Vec<f64>),
}

fn indexed_to_json(value: Indexed) -> Value {
    match value {
        Indexed::Text(text) => Value::String(text),
        Indexed::Number(number) => json!(number),
        Indexed::Texts(items) => Value::Array(items.into_iter().map(Value::String).collect()),
        Indexed::Numbers(items) => {
            Value::Array(items.into_iter().map(|number| json!(number)).collect())
        }
    }
}

fn push_blob(blobs: &mut BTreeMap<String, String>, language: String, value: &Indexed) {
    let text = match value {
        Indexed::Text(text) => text.clone(),
        Indexed::Number(number) => number.to_string(),
        Indexed::Texts(items) => items.join(" "),
        Indexed::Numbers(items) => items
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" "),
    };
    let entry = blobs.entry(language).or_default();
    if !entry.is_empty() {
        entry.push(' ');
    }
    entry.push_str(&text);
}

fn index_value(
    model: &CompiledModel,
    attribute: &braian_model::Attribute,
    value: &Value,
) -> Option<Indexed> {
    if attribute.multiple {
        let items = value.as_array()?;
        if matches!(
            attribute.kind,
            AttributeKind::Number { .. } | AttributeKind::Range { .. }
        ) {
            let numbers = items
                .iter()
                .filter_map(|item| scalar_number(model, attribute, item))
                .collect();
            return Some(Indexed::Numbers(numbers));
        }
        let texts = items.iter().filter_map(scalar_text).collect();
        return Some(Indexed::Texts(texts));
    }
    if matches!(
        attribute.kind,
        AttributeKind::Number { .. } | AttributeKind::Range { .. }
    ) {
        return scalar_number(model, attribute, value).map(Indexed::Number);
    }
    scalar_text(value).map(Indexed::Text)
}

fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Object(object) => object
            .get("value")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| Some(value.to_string())),
        _ => Some(value.to_string()),
    }
}

fn scalar_number(
    model: &CompiledModel,
    attribute: &braian_model::Attribute,
    value: &Value,
) -> Option<f64> {
    let (raw, unit) = if let Some(object) = value.as_object() {
        if object.contains_key("min") {
            let min = object.get("min")?.as_str()?.parse().ok()?;
            let unit = object
                .get("unit")
                .and_then(Value::as_str)
                .map(str::to_owned);
            (min, unit)
        } else {
            let raw = object.get("value")?.as_str()?.parse().ok()?;
            let unit = object
                .get("unit")
                .and_then(Value::as_str)
                .map(str::to_owned);
            (raw, unit)
        }
    } else {
        return None;
    };
    let entered = braian_model::NumberValue { value: raw, unit };
    let normalized = model.normalize(attribute.id, &entered).ok()?;
    normalized.to_f64()
}
