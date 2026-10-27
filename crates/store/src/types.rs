use braian_model::{Object, ObjectId, ObjectPatch};
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::ids::{BatchId, ChangeOp, PrincipalId};

/// Which pieces [`crate::Store::get_objects_projected`] loads.
///
/// A full object is groups plus every latest value part. `parts` limits the
/// value documents when `values` is set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectProjection {
    pub groups: bool,
    pub values: bool,
    pub parts: Option<Vec<String>>,
}

impl ObjectProjection {
    pub fn full() -> Self {
        Self {
            groups: true,
            values: true,
            parts: None,
        }
    }
}

impl Default for ObjectProjection {
    fn default() -> Self {
        Self::full()
    }
}

/// One object as stored, including the fields that are not part of the model object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredObject {
    pub object: Object,
    pub key: String,
    pub version: u64,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// Column stamps that are not part of the object document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectAudit {
    pub id: ObjectId,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub updated_by: Option<PrincipalId>,
    pub updated_by_name: Option<String>,
    pub updated_by_kind: Option<crate::ids::PrincipalKind>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ObjectOp {
    Create(Object),
    /// `read_version` is the version the caller validated against.
    /// `base_version` is the version the caller's edit started from.
    Update {
        id: ObjectId,
        patch: ObjectPatch,
        base_version: u64,
        read_version: u64,
    },
    /// `base_version` conflicts with any change since that version. `None` skips the check.
    Delete {
        id: ObjectId,
        base_version: Option<u64>,
    },
    /// A link object plus the row that points from one object to another.
    CreateLink {
        object: Object,
        from_id: ObjectId,
        to_id: ObjectId,
    },
    /// Values and, when set, a new source or target for an existing link.
    UpdateLink {
        id: ObjectId,
        patch: braian_model::ObjectPatch,
        base_version: u64,
        read_version: u64,
        from_id: Option<ObjectId>,
        to_id: Option<ObjectId>,
    },
}

/// One link as seen from an object. `outgoing` means that object is the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkView {
    pub id: ObjectId,
    pub link_type_id: braian_model::TypeId,
    pub from_id: ObjectId,
    pub to_id: ObjectId,
    pub peer_id: ObjectId,
    pub peer_key: String,
    pub peer_type_id: braian_model::TypeId,
    pub outgoing: bool,
    /// True when the link object stores a value of its own.
    pub has_values: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteReceipt {
    pub batch_id: BatchId,
    pub last_seq: i64,
    pub versions: Vec<(ObjectId, u64)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulkReport {
    pub batch_id: BatchId,
    pub written: u64,
    pub last_seq: i64,
    pub errors: Vec<BulkRowError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulkRowError {
    pub object_id: ObjectId,
    pub key: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrincipalRecord {
    pub id: crate::ids::PrincipalId,
    pub kind: crate::ids::PrincipalKind,
    pub name: String,
    pub handle: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChangeRow {
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub batch_id: BatchId,
    pub object_id: Option<ObjectId>,
    pub version: Option<u64>,
    pub op: ChangeOp,
    pub actor: PrincipalId,
    pub on_behalf_of: Option<PrincipalId>,
    pub diff: Option<Value>,
}

#[derive(Debug, Clone)]
pub(crate) struct CachedDefs {
    pub(crate) set: braian_model::DefinitionSet,
    pub(crate) split: crate::split::SplitContext,
}

impl CachedDefs {
    pub(crate) fn from_groups(
        set: braian_model::DefinitionSet,
        groups: std::collections::HashMap<braian_model::AttributeId, usize>,
    ) -> Self {
        let split = crate::split::SplitContext::with_groups(&set, &groups);
        Self { set, split }
    }

    pub(crate) fn key_of(&self, object: &Object) -> Result<String, crate::error::Error> {
        use crate::error::Error;
        let kind = self
            .set
            .types
            .iter()
            .find(|object_type| object_type.id == object.type_id)
            .ok_or(Error::UnknownType(object.type_id))?;
        match object.values.get(&kind.key_attribute) {
            Some(Value::String(key)) if !key.is_empty() => Ok(key.clone()),
            _ => Err(Error::BadKey(object.id)),
        }
    }
}
