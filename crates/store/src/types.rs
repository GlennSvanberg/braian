use chrono::{DateTime, Utc};
use openpim_model::{Object, ObjectId, ObjectPatch};
use serde_json::Value;

use crate::ids::{BatchId, ChangeOp, PrincipalId};

/// One object as stored, including the fields that are not part of the model object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredObject {
    pub object: Object,
    pub key: String,
    pub version: u64,
    pub deleted_at: Option<DateTime<Utc>>,
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
    pub(crate) set: openpim_model::DefinitionSet,
    pub(crate) split: crate::split::SplitContext,
}

impl CachedDefs {
    pub(crate) fn from_groups(
        set: openpim_model::DefinitionSet,
        groups: std::collections::HashMap<openpim_model::AttributeId, usize>,
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
