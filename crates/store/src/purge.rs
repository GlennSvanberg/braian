//! Counts and removes the objects, values, links and selections a model delete names.
//!
//! The rules match [`braian_model::tally`]. This module is the Postgres version.

use std::collections::{BTreeMap, HashSet};

use braian_model::{ConsequenceCounts, DataPurge, ValueMatch};
use serde_json::Value;
use tokio_postgres::Transaction;
use uuid::Uuid;

use crate::error::Error;
use crate::ids::{Actor, BatchId, ChangeOp};
use crate::ops::insert_change;

use braian_model::ObjectId;

pub async fn measure(
    client: &impl deadpool_postgres::GenericClient,
    purge: &DataPurge,
) -> Result<ConsequenceCounts, Error> {
    if !purge.destructive {
        return Ok(ConsequenceCounts::default());
    }
    let mut counts = ConsequenceCounts::default();
    if !purge.delete_types.is_empty() {
        let types = &purge.delete_types;
        let objects: i64 = client
            .query_one(
                "SELECT count(*)::bigint FROM objects
                 WHERE deleted_at IS NULL AND type_id = ANY($1)",
                &[types],
            )
            .await?
            .get(0);
        let values: i64 = client
            .query_one(
                "SELECT coalesce(sum((SELECT count(*) FROM jsonb_object_keys(v.\"values\"))), 0)::bigint
                 FROM object_values v
                 JOIN objects o ON o.id = v.object_id
                 WHERE v.is_latest AND o.deleted_at IS NULL AND o.type_id = ANY($1)",
                &[types],
            )
            .await?
            .get(0);
        let links: i64 = client
            .query_one(
                "SELECT count(*)::bigint FROM relations r
                 WHERE EXISTS (
                   SELECT 1 FROM objects o
                   WHERE o.deleted_at IS NULL AND o.type_id = ANY($1)
                     AND (o.id = r.from_id OR o.id = r.to_id)
                 )",
                &[types],
            )
            .await?
            .get(0);
        let typed_links: i64 = client
            .query_one(
                "SELECT count(*)::bigint FROM links l
                 WHERE EXISTS (
                   SELECT 1 FROM objects o
                   WHERE o.deleted_at IS NULL AND o.type_id = ANY($1)
                     AND (o.id = l.object_id OR o.id = l.from_id OR o.id = l.to_id)
                 )",
                &[types],
            )
            .await?
            .get(0);
        let cascaded: i64 = client
            .query_one(
                "SELECT count(DISTINCT l.object_id)::bigint
                 FROM links l
                 JOIN objects link_obj ON link_obj.id = l.object_id AND link_obj.deleted_at IS NULL
                 WHERE NOT (link_obj.type_id = ANY($1))
                   AND EXISTS (
                     SELECT 1 FROM objects o
                     WHERE o.deleted_at IS NULL AND o.type_id = ANY($1)
                       AND (o.id = l.from_id OR o.id = l.to_id)
                   )",
                &[types],
            )
            .await?
            .get(0);
        let selections: i64 = client
            .query_one(
                "SELECT count(*)::bigint FROM assortments WHERE type_id = ANY($1)",
                &[types],
            )
            .await?
            .get(0);
        let ref_links = link_values_on_types(client, types, &purge.link_attributes).await?;
        counts.removed += objects + cascaded;
        counts.values += values;
        counts.links += links + typed_links + ref_links;
        counts.selections += selections;
    }
    if !purge.drop_attributes.is_empty() {
        let (objects, values, links) = dropped_values(client, purge).await?;
        counts.removed += objects;
        counts.values += values;
        counts.links += links;
    }
    if !purge.unplace_groups.is_empty() {
        counts.unplaced = unplaced(client, purge).await?;
    }
    counts.objects = counts.removed + counts.unplaced;
    Ok(counts)
}

pub async fn apply(
    tx: &Transaction<'_>,
    actor: &Actor,
    batch: BatchId,
    purge: &DataPurge,
) -> Result<i64, Error> {
    if !purge.destructive {
        return Ok(0);
    }
    let mut last = 0i64;
    if !purge.drop_attributes.is_empty() {
        last = last.max(strip_values(tx, actor, batch, purge).await?);
    }
    if !purge.unplace_groups.is_empty() {
        last = last.max(unplace(tx, actor, batch, purge).await?);
    }
    if !purge.delete_types.is_empty() {
        last = last.max(delete_types(tx, actor, batch, purge).await?);
    }
    Ok(last)
}

async fn link_values_on_types(
    client: &impl deadpool_postgres::GenericClient,
    types: &[Uuid],
    attributes: &[Uuid],
) -> Result<i64, Error> {
    let keys = uuid_keys(attributes);
    if keys.is_empty() {
        return Ok(0);
    }
    let count: i64 = client
        .query_one(
            "SELECT coalesce(sum(
                CASE
                  WHEN jsonb_typeof(e.value) = 'array' THEN jsonb_array_length(e.value)
                  ELSE 1
                END
             ), 0)::bigint
             FROM object_values v
             JOIN objects o ON o.id = v.object_id
             CROSS JOIN LATERAL jsonb_each(v.\"values\") e
             WHERE v.is_latest AND o.deleted_at IS NULL AND o.type_id = ANY($1)
               AND e.key = ANY($2)",
            &[&types, &keys],
        )
        .await?
        .get(0);
    Ok(count)
}

async fn dropped_values(
    client: &impl deadpool_postgres::GenericClient,
    purge: &DataPurge,
) -> Result<(i64, i64, i64), Error> {
    let keys = uuid_keys(&purge.drop_attributes);
    let link_keys = uuid_keys(&purge.link_attributes);
    if purge.match_value.is_some() {
        return dropped_matching(client, purge, &keys, &link_keys).await;
    }
    let row = client
        .query_one(
            "SELECT count(DISTINCT object_id)::bigint,
                    coalesce(sum(n), 0)::bigint,
                    coalesce(sum(links), 0)::bigint
             FROM (
               SELECT o.id AS object_id,
                 (SELECT coalesce(sum(
                    CASE
                      WHEN jsonb_typeof(e.value) = 'array' THEN jsonb_array_length(e.value)
                      WHEN jsonb_typeof(e.value) = 'object'
                           AND NOT (e.value ? 'value') AND NOT (e.value ? 'unit')
                        THEN (SELECT count(*) FROM jsonb_object_keys(e.value))
                      ELSE 1
                    END
                  ), 0)
                  FROM jsonb_each(v.\"values\") e
                  WHERE e.key = ANY($1)) AS n,
                 (SELECT coalesce(sum(
                    CASE
                      WHEN jsonb_typeof(e.value) = 'array' THEN jsonb_array_length(e.value)
                      ELSE 1
                    END
                  ), 0)
                  FROM jsonb_each(v.\"values\") e
                  WHERE e.key = ANY($2)) AS links
               FROM object_values v
               JOIN objects o ON o.id = v.object_id
               WHERE v.is_latest AND o.deleted_at IS NULL
                 AND v.\"values\" ?| $1
                 AND (cardinality($3::uuid[]) = 0 OR NOT (o.type_id = ANY($3)))
                 AND (cardinality($4::uuid[]) = 0 OR o.type_id = ANY($4))
                 AND (cardinality($5::uuid[]) = 0 OR EXISTS (
                   SELECT 1 FROM object_structures s
                   WHERE s.object_id = o.id AND s.group_id = ANY($5)
                 ))
             ) hit
             WHERE n > 0",
            &[
                &keys,
                &link_keys,
                &purge.delete_types,
                &purge.limit_types,
                &purge.limit_groups,
            ],
        )
        .await?;
    Ok((row.get(0), row.get(1), row.get(2)))
}

async fn dropped_matching(
    client: &impl deadpool_postgres::GenericClient,
    purge: &DataPurge,
    keys: &[String],
    link_keys: &[String],
) -> Result<(i64, i64, i64), Error> {
    let rows = client
        .query(
            "SELECT o.id, v.\"values\"
             FROM object_values v
             JOIN objects o ON o.id = v.object_id
             WHERE v.is_latest AND o.deleted_at IS NULL
               AND v.\"values\" ?| $1
               AND (cardinality($2::uuid[]) = 0 OR NOT (o.type_id = ANY($2)))
               AND (cardinality($3::uuid[]) = 0 OR o.type_id = ANY($3))
               AND (cardinality($4::uuid[]) = 0 OR EXISTS (
                 SELECT 1 FROM object_structures s
                 WHERE s.object_id = o.id AND s.group_id = ANY($4)
               ))",
            &[
                &keys,
                &purge.delete_types,
                &purge.limit_types,
                &purge.limit_groups,
            ],
        )
        .await?;
    let key_set: HashSet<&str> = keys.iter().map(String::as_str).collect();
    let link_set: HashSet<&str> = link_keys.iter().map(String::as_str).collect();
    let mut objects = BTreeMap::<Uuid, (i64, i64)>::new();
    for row in rows {
        let id: Uuid = row.get(0);
        let value: Value = row.get(1);
        let (removed, links) = count_match(&value, &key_set, &link_set, purge.match_value.as_ref());
        if removed > 0 {
            let entry = objects.entry(id).or_insert((0, 0));
            entry.0 += removed;
            entry.1 += links;
        }
    }
    let values = objects.values().map(|item| item.0).sum();
    let links = objects.values().map(|item| item.1).sum();
    let object_count = i64::try_from(objects.len()).unwrap_or(i64::MAX);
    Ok((object_count, values, links))
}

async fn unplaced(
    client: &impl deadpool_postgres::GenericClient,
    purge: &DataPurge,
) -> Result<i64, Error> {
    let keys = uuid_keys(&purge.drop_attributes);
    let count: i64 = client
        .query_one(
            "SELECT count(DISTINCT o.id)::bigint
             FROM objects o
             JOIN object_structures s ON s.object_id = o.id
             WHERE o.deleted_at IS NULL
               AND s.group_id = ANY($1)
               AND (cardinality($2::uuid[]) = 0 OR NOT (o.type_id = ANY($2)))
               AND NOT EXISTS (
                 SELECT 1 FROM object_values v
                 WHERE v.object_id = o.id AND v.is_latest
                   AND cardinality($3::text[]) > 0
                   AND v.\"values\" ?| $3
               )",
            &[&purge.unplace_groups, &purge.delete_types, &keys],
        )
        .await?
        .get(0);
    Ok(count)
}

async fn strip_values(
    tx: &Transaction<'_>,
    actor: &Actor,
    batch: BatchId,
    purge: &DataPurge,
) -> Result<i64, Error> {
    let keys = uuid_keys(&purge.drop_attributes);
    let link_keys = uuid_keys(&purge.link_attributes);
    let rows = tx
        .query(
            "SELECT o.id, v.part, v.\"values\"
             FROM object_values v
             JOIN objects o ON o.id = v.object_id
             WHERE v.is_latest AND o.deleted_at IS NULL
               AND v.\"values\" ?| $1
               AND (cardinality($2::uuid[]) = 0 OR NOT (o.type_id = ANY($2)))
               AND (cardinality($3::uuid[]) = 0 OR o.type_id = ANY($3))
               AND (cardinality($4::uuid[]) = 0 OR EXISTS (
                 SELECT 1 FROM object_structures s
                 WHERE s.object_id = o.id AND s.group_id = ANY($4)
               ))",
            &[
                &keys,
                &purge.delete_types,
                &purge.limit_types,
                &purge.limit_groups,
            ],
        )
        .await?;
    let key_set: HashSet<String> = keys.into_iter().collect();
    let link_set: HashSet<String> = link_keys.into_iter().collect();
    let mut by_object: BTreeMap<Uuid, Vec<(String, Value)>> = BTreeMap::new();
    for row in rows {
        let id: Uuid = row.get(0);
        by_object
            .entry(id)
            .or_default()
            .push((row.get(1), row.get(2)));
    }
    let mut last = 0i64;
    for (object_id, parts) in by_object {
        let mut rewritten = Vec::new();
        for (part, mut value) in parts {
            let key_refs: HashSet<&str> = key_set.iter().map(String::as_str).collect();
            let link_refs: HashSet<&str> = link_set.iter().map(String::as_str).collect();
            let (removed, _) = strip_match(
                &mut value,
                &key_refs,
                &link_refs,
                purge.match_value.as_ref(),
            );
            if removed > 0 {
                rewritten.push((part, value));
            }
        }
        if rewritten.is_empty() {
            continue;
        }
        let version: i64 = tx
            .query_one(
                "UPDATE objects SET version = version + 1, updated_at = now()
                 WHERE id = $1 RETURNING version",
                &[&object_id],
            )
            .await?
            .get(0);
        for (part, value) in rewritten {
            tx.execute(
                "UPDATE object_values SET is_latest = false
                 WHERE object_id = $1 AND part = $2 AND is_latest",
                &[&object_id, &part],
            )
            .await?;
            tx.execute(
                "INSERT INTO object_values (object_id, part, version, \"values\", is_latest)
                 VALUES ($1, $2, $3, $4, true)",
                &[&object_id, &part, &version, &value],
            )
            .await?;
        }
        let seq = insert_change(
            tx,
            batch,
            Some(ObjectId::from_uuid(object_id)),
            Some(version),
            ChangeOp::Update,
            actor,
            &serde_json::json!({ "$removed_attributes": key_set.iter().collect::<Vec<_>>() }),
        )
        .await?;
        last = last.max(seq);
    }
    Ok(last)
}

async fn unplace(
    tx: &Transaction<'_>,
    actor: &Actor,
    batch: BatchId,
    purge: &DataPurge,
) -> Result<i64, Error> {
    let rows = tx
        .query(
            "DELETE FROM object_structures s
             USING objects o
             WHERE s.object_id = o.id AND o.deleted_at IS NULL
               AND s.group_id = ANY($1)
               AND (cardinality($2::uuid[]) = 0 OR NOT (o.type_id = ANY($2)))
             RETURNING s.object_id",
            &[&purge.unplace_groups, &purge.delete_types],
        )
        .await?;
    let mut seen = BTreeMap::<Uuid, ()>::new();
    for row in rows {
        seen.insert(row.get(0), ());
    }
    let mut last = 0i64;
    for object_id in seen.into_keys() {
        let version: i64 = tx
            .query_one(
                "UPDATE objects SET version = version + 1, updated_at = now()
                 WHERE id = $1 RETURNING version",
                &[&object_id],
            )
            .await?
            .get(0);
        let seq = insert_change(
            tx,
            batch,
            Some(ObjectId::from_uuid(object_id)),
            Some(version),
            ChangeOp::Update,
            actor,
            &serde_json::json!({ "$unplaced": true }),
        )
        .await?;
        last = last.max(seq);
    }
    Ok(last)
}

async fn delete_types(
    tx: &Transaction<'_>,
    actor: &Actor,
    batch: BatchId,
    purge: &DataPurge,
) -> Result<i64, Error> {
    let types = &purge.delete_types;
    let extra = tx
        .query(
            "SELECT DISTINCT l.object_id
             FROM links l
             JOIN objects link_obj ON link_obj.id = l.object_id AND link_obj.deleted_at IS NULL
             WHERE NOT (link_obj.type_id = ANY($1))
               AND EXISTS (
                 SELECT 1 FROM objects o
                 WHERE o.deleted_at IS NULL AND o.type_id = ANY($1)
                   AND (o.id = l.from_id OR o.id = l.to_id)
               )",
            &[types],
        )
        .await?;
    let extra_ids: Vec<Uuid> = extra.iter().map(|row| row.get(0)).collect();
    tx.execute(
        "DELETE FROM links l
         WHERE l.object_id = ANY($2)
            OR EXISTS (
              SELECT 1 FROM objects o
              WHERE o.deleted_at IS NULL AND o.type_id = ANY($1)
                AND (o.id = l.object_id OR o.id = l.from_id OR o.id = l.to_id)
            )",
        &[types, &extra_ids],
    )
    .await?;
    tx.execute(
        "DELETE FROM relations r
         WHERE EXISTS (
           SELECT 1 FROM objects o
           WHERE o.deleted_at IS NULL AND o.type_id = ANY($1)
             AND (o.id = r.from_id OR o.id = r.to_id)
         )",
        &[types],
    )
    .await?;
    tx.execute(
        "UPDATE job_types SET assortment_id = NULL
         WHERE assortment_id IN (SELECT id FROM assortments WHERE type_id = ANY($1))",
        &[types],
    )
    .await?;
    tx.execute(
        "UPDATE webhooks SET assortment_id = NULL
         WHERE assortment_id IN (SELECT id FROM assortments WHERE type_id = ANY($1))",
        &[types],
    )
    .await?;
    tx.execute(
        "UPDATE job_runs SET assortment_id = NULL
         WHERE assortment_id IN (SELECT id FROM assortments WHERE type_id = ANY($1))",
        &[types],
    )
    .await?;
    tx.execute("DELETE FROM assortments WHERE type_id = ANY($1)", &[types])
        .await?;
    let mut last = 0i64;
    if !extra_ids.is_empty() {
        let cascaded = tx
            .query(
                "UPDATE objects
                 SET version = version + 1, updated_at = now(), deleted_at = now()
                 WHERE deleted_at IS NULL AND id = ANY($1)
                 RETURNING id, version",
                &[&extra_ids],
            )
            .await?;
        for row in cascaded {
            let id: Uuid = row.get(0);
            let version: i64 = row.get(1);
            let seq = insert_change(
                tx,
                batch,
                Some(ObjectId::from_uuid(id)),
                Some(version),
                ChangeOp::Delete,
                actor,
                &serde_json::json!({ "link_removed": true }),
            )
            .await?;
            last = last.max(seq);
        }
    }
    let rows = tx
        .query(
            "UPDATE objects
             SET version = version + 1, updated_at = now(), deleted_at = now()
             WHERE deleted_at IS NULL AND type_id = ANY($1)
             RETURNING id, version",
            &[types],
        )
        .await?;
    for row in rows {
        let id: Uuid = row.get(0);
        let version: i64 = row.get(1);
        let seq = insert_change(
            tx,
            batch,
            Some(ObjectId::from_uuid(id)),
            Some(version),
            ChangeOp::Delete,
            actor,
            &serde_json::json!({ "type_deleted": true }),
        )
        .await?;
        last = last.max(seq);
    }
    Ok(last)
}

fn uuid_keys(ids: &[Uuid]) -> Vec<String> {
    ids.iter().map(ToString::to_string).collect()
}

fn count_match(
    value: &Value,
    keys: &HashSet<&str>,
    link_keys: &HashSet<&str>,
    filter: Option<&ValueMatch>,
) -> (i64, i64) {
    let Some(object) = value.as_object() else {
        return (0, 0);
    };
    let mut removed = 0i64;
    let mut links = 0i64;
    for (key, current) in object {
        if !keys.contains(key.as_str()) || !passes(current, filter) {
            continue;
        }
        let n = count_entries(current);
        removed += n;
        if link_keys.contains(key.as_str()) {
            links += n;
        }
    }
    (removed, links)
}

fn strip_match(
    value: &mut Value,
    keys: &HashSet<&str>,
    link_keys: &HashSet<&str>,
    filter: Option<&ValueMatch>,
) -> (i64, i64) {
    let counts = count_match(value, keys, link_keys, filter);
    let Some(object) = value.as_object_mut() else {
        return (0, 0);
    };
    object.retain(|key, current| !(keys.contains(key.as_str()) && passes(current, filter)));
    counts
}

fn passes(value: &Value, filter: Option<&ValueMatch>) -> bool {
    match filter {
        None => true,
        Some(filter) => matches_filter(value, filter),
    }
}

fn matches_filter(value: &Value, filter: &ValueMatch) -> bool {
    match filter {
        ValueMatch::Code(code) => contains_code(value, code),
        ValueMatch::Unit(unit) => contains_unit(value, unit),
    }
}

fn contains_code(value: &Value, code: &str) -> bool {
    match value {
        Value::String(text) => text == code,
        Value::Array(items) => items.iter().any(|item| contains_code(item, code)),
        Value::Object(map) => map.values().any(|item| contains_code(item, code)),
        _ => false,
    }
}

fn contains_unit(value: &Value, unit: &str) -> bool {
    match value {
        Value::Object(map) => {
            map.get("unit").and_then(Value::as_str) == Some(unit)
                || map.values().any(|item| contains_unit(item, unit))
        }
        Value::Array(items) => items.iter().any(|item| contains_unit(item, unit)),
        _ => false,
    }
}

fn count_entries(value: &Value) -> i64 {
    match value {
        Value::Array(items) => i64::try_from(items.len()).unwrap_or(i64::MAX),
        Value::Object(map)
            if !map.contains_key("value") && !map.contains_key("unit") && !map.is_empty() =>
        {
            i64::try_from(map.len()).unwrap_or(i64::MAX)
        }
        Value::Null => 0,
        _ => 1,
    }
}
