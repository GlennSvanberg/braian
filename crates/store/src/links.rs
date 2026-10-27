//! Link rows. A link is also an object; this table is the two ends.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use braian_model::{Cardinality, DefinitionSet, ObjectId, TypeId};
use tokio_postgres::Transaction;
use uuid::Uuid;

use crate::error::Error;
use crate::types::LinkView;

pub async fn links_for(
    client: &impl tokio_postgres::GenericClient,
    id: ObjectId,
) -> Result<Vec<LinkView>, Error> {
    let rows = client
        .query(
            "SELECT l.object_id, l.link_type_id, l.from_id, l.to_id,
                    CASE WHEN l.from_id = $1 THEN l.to_id ELSE l.from_id END AS peer_id,
                    peer.key AS peer_key,
                    peer.type_id AS peer_type_id,
                    (l.from_id = $1) AS outgoing
             FROM links l
             JOIN objects self ON self.id = l.object_id AND self.deleted_at IS NULL
             JOIN objects peer ON peer.id = CASE WHEN l.from_id = $1 THEN l.to_id ELSE l.from_id END
               AND peer.deleted_at IS NULL
             WHERE l.from_id = $1 OR l.to_id = $1
             ORDER BY l.link_type_id, outgoing DESC, peer.key",
            &[&id.as_uuid()],
        )
        .await?;
    let ids: Vec<Uuid> = rows.iter().map(|row| row.get("object_id")).collect();
    let valued = valued_ids(client, &ids).await?;
    let mut views = Vec::with_capacity(rows.len());
    for row in &rows {
        let object_id: Uuid = row.get("object_id");
        views.push(LinkView {
            id: ObjectId::from_uuid(object_id),
            link_type_id: TypeId::from_uuid(row.get("link_type_id")),
            from_id: ObjectId::from_uuid(row.get("from_id")),
            to_id: ObjectId::from_uuid(row.get("to_id")),
            peer_id: ObjectId::from_uuid(row.get("peer_id")),
            peer_key: row.get::<_, Option<String>>("peer_key").unwrap_or_default(),
            peer_type_id: TypeId::from_uuid(row.get("peer_type_id")),
            outgoing: row.get("outgoing"),
            has_values: valued.contains(&object_id),
        });
    }
    Ok(views)
}

pub async fn delete_counts(
    client: &impl tokio_postgres::GenericClient,
    id: ObjectId,
) -> Result<(i64, i64), Error> {
    let row = client
        .query_one(
            "SELECT count(*)::bigint,
                    count(*) FILTER (WHERE l.object_id <> $1)::bigint
             FROM links l
             JOIN objects self ON self.id = l.object_id AND self.deleted_at IS NULL
             WHERE l.object_id = $1 OR l.from_id = $1 OR l.to_id = $1",
            &[&id.as_uuid()],
        )
        .await?;
    Ok((row.get(0), row.get(1)))
}

pub async fn count_touching(
    client: &impl tokio_postgres::GenericClient,
    id: ObjectId,
) -> Result<i64, Error> {
    let count: i64 = client
        .query_one(
            "SELECT count(*)::bigint FROM links l
             JOIN objects self ON self.id = l.object_id AND self.deleted_at IS NULL
             WHERE l.object_id = $1 OR l.from_id = $1 OR l.to_id = $1",
            &[&id.as_uuid()],
        )
        .await?
        .get(0);
    Ok(count)
}

/// Copies cardinality onto the rows so the partial unique indexes can enforce `one`.
pub async fn sync_cardinality(tx: &Transaction<'_>, defs: &DefinitionSet) -> Result<(), Error> {
    for object_type in &defs.types {
        if !object_type.is_link() {
            continue;
        }
        let Some(spec) = object_type.link.as_ref() else {
            continue;
        };
        let from_one = spec.from_cardinality == Cardinality::One;
        let to_one = spec.to_cardinality == Cardinality::One;
        tx.execute(
            "UPDATE links SET from_one = $2, to_one = $3 WHERE link_type_id = $1",
            &[&object_type.id.as_uuid(), &from_one, &to_one],
        )
        .await
        .map_err(|error| map_cardinality(error, &object_type.key))?;
    }
    Ok(())
}

pub async fn touching_ids(tx: &Transaction<'_>, id: ObjectId) -> Result<Vec<Uuid>, Error> {
    let rows = tx
        .query(
            "SELECT object_id FROM links WHERE object_id = $1 OR from_id = $1 OR to_id = $1",
            &[&id.as_uuid()],
        )
        .await?;
    Ok(rows.iter().map(|row| row.get(0)).collect())
}

pub async fn delete_rows(tx: &Transaction<'_>, ids: &[Uuid]) -> Result<(), Error> {
    if ids.is_empty() {
        return Ok(());
    }
    tx.execute("DELETE FROM links WHERE object_id = ANY($1)", &[&ids])
        .await?;
    Ok(())
}

pub async fn endpoint_type(tx: &Transaction<'_>, id: ObjectId) -> Result<TypeId, Error> {
    let row = tx
        .query_opt(
            "SELECT type_id, deleted_at FROM objects WHERE id = $1",
            &[&id.as_uuid()],
        )
        .await?;
    let Some(row) = row else {
        return Err(Error::NotFound(id));
    };
    let deleted_at: Option<chrono::DateTime<chrono::Utc>> = row.get("deleted_at");
    if deleted_at.is_some() {
        return Err(Error::Deleted(id));
    }
    Ok(TypeId::from_uuid(row.get("type_id")))
}

pub async fn existing_counts(
    tx: &Transaction<'_>,
    link_type: TypeId,
    from_id: ObjectId,
    to_id: ObjectId,
    except: Option<ObjectId>,
) -> Result<(i64, i64), Error> {
    let except = except.map(ObjectId::as_uuid);
    let from_count: i64 = tx
        .query_one(
            "SELECT count(*)::bigint FROM links
             WHERE link_type_id = $1 AND from_id = $2
               AND ($3::uuid IS NULL OR object_id <> $3)",
            &[&link_type.as_uuid(), &from_id.as_uuid(), &except],
        )
        .await?
        .get(0);
    let to_count: i64 = tx
        .query_one(
            "SELECT count(*)::bigint FROM links
             WHERE link_type_id = $1 AND to_id = $2
               AND ($3::uuid IS NULL OR object_id <> $3)",
            &[&link_type.as_uuid(), &to_id.as_uuid(), &except],
        )
        .await?
        .get(0);
    Ok((from_count, to_count))
}

pub fn map_cardinality(error: tokio_postgres::Error, label: &str) -> Error {
    if let Some(db) = error.as_db_error()
        && db.code() == &tokio_postgres::error::SqlState::UNIQUE_VIOLATION
        && matches!(db.constraint(), Some("links_one_from" | "links_one_to"))
    {
        return Error::LinkCardinality(format!("link type {label} allows one link on that side"));
    }
    Error::Db(error)
}

impl crate::session::Store {
    /// Peer numbers for each end in `ids`, keyed by link type id and direction.
    pub async fn link_peer_index(
        &self,
        ids: &[Uuid],
    ) -> Result<HashMap<Uuid, BTreeMap<(Uuid, bool), BTreeSet<String>>>, Error> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT l.link_type_id, l.from_id AS endpoint, true AS outgoing, COALESCE(peer.key, '') AS peer_key
                 FROM links l
                 JOIN objects link_obj ON link_obj.id = l.object_id AND link_obj.deleted_at IS NULL
                 JOIN objects peer ON peer.id = l.to_id AND peer.deleted_at IS NULL
                 WHERE l.from_id = ANY($1)
                 UNION ALL
                 SELECT l.link_type_id, l.to_id, false, COALESCE(peer.key, '')
                 FROM links l
                 JOIN objects link_obj ON link_obj.id = l.object_id AND link_obj.deleted_at IS NULL
                 JOIN objects peer ON peer.id = l.from_id AND peer.deleted_at IS NULL
                 WHERE l.to_id = ANY($1)",
                &[&ids],
            )
            .await?;
        let mut index = HashMap::new();
        for row in rows {
            let endpoint: Uuid = row.get(1);
            let link_type: Uuid = row.get(0);
            let outgoing: bool = row.get(2);
            let key: String = row.get(3);
            index
                .entry(endpoint)
                .or_insert_with(BTreeMap::new)
                .entry((link_type, outgoing))
                .or_insert_with(BTreeSet::new)
                .insert(key);
        }
        Ok(index)
    }

    pub async fn links_for(&self, id: ObjectId) -> Result<Vec<LinkView>, Error> {
        let client = self.pool().get().await?;
        links_for(&**client, id).await
    }

    pub async fn count_links_touching(&self, id: ObjectId) -> Result<i64, Error> {
        let client = self.pool().get().await?;
        count_touching(&**client, id).await
    }

    /// True when a live link of this type already joins the pair, or a `one` side is taken.
    pub async fn link_blocks(
        &self,
        link_type: TypeId,
        from_id: ObjectId,
        to_id: ObjectId,
        from_one: bool,
        to_one: bool,
    ) -> Result<bool, Error> {
        let client = self.pool().get().await?;
        let blocked: bool = client
            .query_one(
                "SELECT EXISTS (
                    SELECT 1
                    FROM links l
                    JOIN objects object ON object.id = l.object_id AND object.deleted_at IS NULL
                    WHERE l.link_type_id = $1
                      AND (
                            (l.from_id = $2 AND l.to_id = $3)
                         OR ($4 AND l.from_id = $2)
                         OR ($5 AND l.to_id = $3)
                      )
                )",
                &[
                    &link_type.as_uuid(),
                    &from_id.as_uuid(),
                    &to_id.as_uuid(),
                    &from_one,
                    &to_one,
                ],
            )
            .await?
            .get(0);
        Ok(blocked)
    }

    /// `(link rows removed, other objects removed)`.
    pub async fn link_delete_counts(&self, id: ObjectId) -> Result<(i64, i64), Error> {
        let client = self.pool().get().await?;
        delete_counts(&**client, id).await
    }
}

async fn valued_ids(
    client: &impl tokio_postgres::GenericClient,
    ids: &[Uuid],
) -> Result<std::collections::HashSet<Uuid>, Error> {
    if ids.is_empty() {
        return Ok(std::collections::HashSet::new());
    }
    let rows = client
        .query(
            "SELECT DISTINCT v.object_id
             FROM object_values v
             WHERE v.is_latest AND v.object_id = ANY($1)
               AND EXISTS (SELECT 1 FROM jsonb_object_keys(v.\"values\"))",
            &[&ids],
        )
        .await?;
    Ok(rows.iter().map(|row| row.get(0)).collect())
}
