//! Search interface. Typesense is one implementation; callers do not talk to it.
//!
//! Spike 2 stores one document per object and one field per language. Numbers
//! also get the value in the family's base unit, as a float used only for
//! sorting and range filters. The exact decimal stays in Postgres.

use std::collections::{BTreeMap, HashMap};

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
    /// Separate pool from [`Self::client`]. Bulk imports keep their connections
    /// until Typesense finishes the batch; interactive upserts use this one.
    interactive: reqwest::Client,
    base: String,
    api_key: String,
    alias: String,
}

fn http_client(idle_per_host: usize) -> reqwest::Client {
    reqwest::Client::builder()
        .pool_max_idle_per_host(idle_per_host)
        .http1_only()
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

impl Typesense {
    pub fn new(base: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            client: http_client(8),
            interactive: http_client(2),
            base: base.into().trim_end_matches('/').to_owned(),
            api_key: api_key.into(),
            alias: ALIAS.to_owned(),
        }
    }

    /// Sends every request on the interactive connection pool.
    ///
    /// A bulk `documents/import` holds its HTTP connection for the whole batch.
    /// Upserts on this handle do not wait for that connection to come free.
    pub fn interactive_writer(&self) -> Self {
        let mut writer = self.clone();
        writer.client = self.interactive.clone();
        writer
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

    /// Imports documents without letting an older `seq` replace a newer one.
    ///
    /// New ids are created in one batch. An id that is already stored is
    /// deleted only when `seq` is greater than the stored value, then created
    /// again, so the write is a full document and still loses to a newer seq.
    pub async fn import(&self, collection: &str, docs: &[Value]) -> Result<(), Error> {
        if docs.is_empty() {
            return Ok(());
        }
        let docs = prefer_newer(docs);
        let ids: Vec<String> = docs.iter().filter_map(doc_id).collect();
        let stored = self.stored_seqs(collection, &ids).await?;
        let plan = plan_import(&stored, &docs);
        let creates: Vec<Value> = plan
            .create
            .iter()
            .map(|index| docs[*index].clone())
            .collect();
        let mut updates: Vec<Value> = plan
            .update
            .iter()
            .map(|index| docs[*index].clone())
            .collect();
        let created = self.import_action(collection, &creates, "create").await?;
        for (doc, line) in creates.into_iter().zip(created) {
            if line.already_exists {
                updates.push(doc);
            }
        }
        if updates.is_empty() {
            return Ok(());
        }
        let mut pending = tokio::task::JoinSet::new();
        for doc in updates {
            let search = self.clone();
            let collection = collection.to_owned();
            pending.spawn(async move { search.replace_if_newer(&collection, &doc).await });
            if pending.len() >= 8
                && let Some(joined) = pending.join_next().await
            {
                joined.map_err(|error| Error::Typesense(error.to_string()))??;
            }
        }
        while let Some(joined) = pending.join_next().await {
            joined.map_err(|error| Error::Typesense(error.to_string()))??;
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

    async fn import_action(
        &self,
        collection: &str,
        docs: &[Value],
        action: &str,
    ) -> Result<Vec<ImportLine>, Error> {
        if docs.is_empty() {
            return Ok(Vec::new());
        }
        let mut body = String::new();
        for doc in docs {
            body.push_str(&serde_json::to_string(doc)?);
            body.push('\n');
        }
        let response = self
            .client
            .post(format!(
                "{}/collections/{collection}/documents/import?action={action}",
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
        if !status.is_success() {
            return Err(Error::Typesense(format!("import {status}: {text}")));
        }
        let mut lines = Vec::new();
        for line in text.lines().filter(|line| !line.is_empty()) {
            let parsed: Value = serde_json::from_str(line).unwrap_or(Value::Null);
            let success = parsed
                .get("success")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let error = parsed
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let already_exists = error.to_ascii_lowercase().contains("already exists");
            if !success && !already_exists {
                return Err(Error::Typesense(format!("import {status}: {line}")));
            }
            lines.push(ImportLine {
                already_exists: !success && already_exists,
            });
        }
        Ok(lines)
    }

    async fn stored_seqs(
        &self,
        collection: &str,
        ids: &[String],
    ) -> Result<HashMap<String, i64>, Error> {
        let mut stored = HashMap::new();
        for chunk in ids.chunks(80) {
            if chunk.is_empty() {
                continue;
            }
            let list = chunk
                .iter()
                .map(|id| format!("`{id}`"))
                .collect::<Vec<_>>()
                .join(",");
            let filter = format!("id:[{list}]");
            let response = self
                .client
                .get(format!(
                    "{}/collections/{collection}/documents/search",
                    self.base
                ))
                .header("X-TYPESENSE-API-KEY", &self.api_key)
                .query(&[
                    ("q", "*"),
                    ("query_by", "key"),
                    ("filter_by", filter.as_str()),
                    ("include_fields", "id,seq"),
                    ("per_page", "250"),
                ])
                .send()
                .await
                .map_err(|error| Error::Typesense(error.to_string()))?;
            let status = response.status();
            let body: Value = response
                .json()
                .await
                .map_err(|error| Error::Typesense(error.to_string()))?;
            if !status.is_success() {
                return Err(Error::Typesense(format!("seq lookup {status}: {body}")));
            }
            let Some(hits) = body.get("hits").and_then(Value::as_array) else {
                continue;
            };
            for hit in hits {
                let Some(document) = hit.get("document") else {
                    continue;
                };
                let Some(id) = doc_id(document) else {
                    continue;
                };
                if let Some(seq) = doc_seq(document) {
                    stored.insert(id, seq);
                }
            }
        }
        Ok(stored)
    }

    async fn stored_seq(&self, collection: &str, id: &str) -> Result<Option<i64>, Error> {
        let response = self
            .client
            .get(format!(
                "{}/collections/{collection}/documents/{id}",
                self.base
            ))
            .header("X-TYPESENSE-API-KEY", &self.api_key)
            .send()
            .await
            .map_err(|error| Error::Typesense(error.to_string()))?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let status = response.status();
        let body: Value = response
            .json()
            .await
            .map_err(|error| Error::Typesense(error.to_string()))?;
        if !status.is_success() {
            return Err(Error::Typesense(format!("read {id}: {status}: {body}")));
        }
        Ok(doc_seq(&body))
    }

    async fn delete_where(&self, collection: &str, filter: &str) -> Result<u64, Error> {
        let response = self
            .client
            .delete(format!("{}/collections/{collection}/documents", self.base))
            .header("X-TYPESENSE-API-KEY", &self.api_key)
            .query(&[("filter_by", filter)])
            .send()
            .await
            .map_err(|error| Error::Typesense(error.to_string()))?;
        let status = response.status();
        let body: Value = response
            .json()
            .await
            .map_err(|error| Error::Typesense(error.to_string()))?;
        if !status.is_success() {
            return Err(Error::Typesense(format!("delete where {status}: {body}")));
        }
        Ok(body.get("num_deleted").and_then(Value::as_u64).unwrap_or(0))
    }

    /// Deletes the stored document when its seq is older, then creates `doc`.
    /// A concurrent newer write is left in place.
    async fn replace_if_newer(&self, collection: &str, doc: &Value) -> Result<(), Error> {
        let (Some(id), Some(seq)) = (doc_id(doc), doc_seq(doc)) else {
            self.import_action(collection, std::slice::from_ref(doc), "upsert")
                .await?;
            return Ok(());
        };
        for _ in 0..3 {
            let deleted = self
                .delete_where(collection, &format!("id:=`{id}` && seq:<{seq}"))
                .await?;
            let stored = self.stored_seq(collection, &id).await?;
            if deleted == 0 && stored.is_some_and(|current| current >= seq) {
                return Ok(());
            }
            if deleted == 0 && stored.is_some() {
                continue;
            }
            let lines = self
                .import_action(collection, std::slice::from_ref(doc), "create")
                .await?;
            if lines.first().is_none_or(|line| !line.already_exists) {
                return Ok(());
            }
        }
        Ok(())
    }
}

struct ImportLine {
    already_exists: bool,
}

/// What to do with one document given the seq already stored for its id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeqDecision {
    Create,
    Update,
    Skip,
}

pub fn seq_decision(stored: Option<i64>, incoming: i64) -> SeqDecision {
    match stored {
        None => SeqDecision::Create,
        Some(stored) if incoming > stored => SeqDecision::Update,
        Some(_) => SeqDecision::Skip,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportPlan {
    pub create: Vec<usize>,
    pub update: Vec<usize>,
    pub skip: Vec<usize>,
}

/// Indexes into `docs`. A missing id is created. A missing seq on an existing
/// id is updated, because there is no version to compare.
pub fn plan_import(stored: &HashMap<String, i64>, docs: &[Value]) -> ImportPlan {
    let mut plan = ImportPlan {
        create: Vec::new(),
        update: Vec::new(),
        skip: Vec::new(),
    };
    for (index, doc) in docs.iter().enumerate() {
        let Some(id) = doc_id(doc) else {
            plan.create.push(index);
            continue;
        };
        let decision = match stored.get(&id).copied() {
            None => SeqDecision::Create,
            Some(stored_seq) => match doc_seq(doc) {
                Some(incoming) => seq_decision(Some(stored_seq), incoming),
                None => SeqDecision::Update,
            },
        };
        match decision {
            SeqDecision::Create => plan.create.push(index),
            SeqDecision::Update => plan.update.push(index),
            SeqDecision::Skip => plan.skip.push(index),
        }
    }
    plan
}

fn doc_id(doc: &Value) -> Option<String> {
    doc.get("id").and_then(Value::as_str).map(str::to_owned)
}

fn doc_seq(doc: &Value) -> Option<i64> {
    doc.get("seq").and_then(Value::as_i64)
}

/// One document per id, keeping the highest seq. Documents without an id stay.
fn prefer_newer(docs: &[Value]) -> Vec<Value> {
    let mut best: HashMap<String, usize> = HashMap::new();
    let mut out = Vec::new();
    for doc in docs {
        let Some(id) = doc_id(doc) else {
            out.push(doc.clone());
            continue;
        };
        if let Some(index) = best.get(&id).copied() {
            let previous = doc_seq(&out[index]).unwrap_or(i64::MIN);
            let incoming = doc_seq(doc).unwrap_or(i64::MIN);
            if incoming >= previous {
                out[index] = doc.clone();
            }
            continue;
        }
        best.insert(id, out.len());
        out.push(doc.clone());
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_older_seq_does_not_replace_a_newer_document() {
        assert_eq!(seq_decision(Some(10), 4), SeqDecision::Skip);
        assert_eq!(seq_decision(Some(10), 10), SeqDecision::Skip);
        assert_eq!(seq_decision(Some(10), 11), SeqDecision::Update);
        assert_eq!(seq_decision(None, 1), SeqDecision::Create);

        let mut stored = HashMap::new();
        stored.insert("obj".into(), 10);
        let docs = vec![
            json!({"id": "obj", "seq": 4}),
            json!({"id": "obj", "seq": 12}),
            json!({"id": "fresh", "seq": 1}),
        ];
        let plan = plan_import(&stored, &docs);
        assert_eq!(plan.skip, vec![0]);
        assert_eq!(plan.update, vec![1]);
        assert_eq!(plan.create, vec![2]);
    }

    #[test]
    fn the_same_id_keeps_only_the_highest_seq() {
        let docs = vec![
            json!({"id": "obj", "seq": 3, "key": "old"}),
            json!({"id": "obj", "seq": 9, "key": "new"}),
            json!({"id": "obj", "seq": 5, "key": "mid"}),
            json!({"id": "other", "seq": 1}),
        ];
        let kept = prefer_newer(&docs);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0]["seq"], 9);
        assert_eq!(kept[0]["key"], "new");
        assert_eq!(kept[1]["id"], "other");
    }
}
