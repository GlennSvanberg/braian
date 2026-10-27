use braian_model::{AttributeId, LanguageTag, ObjectId, StructureId, TypeId};
use serde_json::Value;
use thiserror::Error;

use crate::ids::PrincipalId;

/// One attribute, language or placement that changed since the caller's base version.
#[derive(Debug, Clone, PartialEq)]
pub struct Conflict {
    pub attribute: Option<AttributeId>,
    pub language: Option<LanguageTag>,
    pub structure: Option<StructureId>,
    pub version: u64,
    pub actor: PrincipalId,
    pub on_behalf_of: Option<PrincipalId>,
    /// Value after the conflicting change. `Null` means it was cleared.
    pub current: Option<Value>,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("database: {0}")]
    Db(#[from] tokio_postgres::Error),
    #[error("connection pool: {0}")]
    Pool(#[from] deadpool_postgres::PoolError),
    #[error("could not build the connection pool: {0}")]
    PoolBuild(#[from] deadpool_postgres::BuildError),
    #[error("migration: {0}")]
    Migrate(String),
    #[error("database url: {0}")]
    Url(String),
    #[error("agent {0} requires on_behalf_of")]
    AgentRequiresBehalf(PrincipalId),
    #[error("unknown principal {0}")]
    UnknownPrincipal(PrincipalId),
    #[error("duplicate key {key} for type {type_id}")]
    DuplicateKey { type_id: TypeId, key: String },
    #[error("{0}")]
    LinkRejected(String),
    #[error("{0}")]
    LinkCardinality(String),
    #[error("object {0} was not found")]
    NotFound(ObjectId),
    #[error("object {0} is deleted")]
    Deleted(ObjectId),
    #[error("conflict at version {current_version}")]
    Conflict {
        current_version: u64,
        conflicts: Vec<Conflict>,
    },
    #[error("object changed since it was read (version {current_version})")]
    StaleRead { current_version: u64 },
    #[error("base version is ahead of {current_version}")]
    BaseAhead { current_version: u64 },
    #[error("object {0} has no string key")]
    BadKey(ObjectId),
    #[error("unknown type {0}")]
    UnknownType(TypeId),
    #[error("no definitions stored")]
    NoDefinitions,
    #[error("definition revision {got} does not follow {current}")]
    BadRevision { current: u64, got: u64 },
    #[error("definitions: {0}")]
    Definitions(String),
    #[error("stored object {0} is corrupt: {1}")]
    Corrupt(ObjectId, String),
    #[error("{0}")]
    Data(String),
    #[error("bulk chunk size must be at least 1")]
    EmptyChunk,
    #[error("index claim on {lane} from {from_seq} is no longer held")]
    ClaimLost { lane: String, from_seq: i64 },
    #[error("proposal {0} was not found")]
    ProposalNotFound(uuid::Uuid),
    #[error("proposal {id} cannot move from {from} to {to}")]
    ProposalTransition {
        id: uuid::Uuid,
        from: String,
        to: String,
    },
    /// Another worker holds a live lease on this proposal.
    #[error("proposal {0} is already being applied")]
    ProposalBusy(uuid::Uuid),
    /// The lease expired or another worker took the proposal.
    #[error("proposal {0} is no longer held")]
    ProposalClaimLost(uuid::Uuid),
}

impl Error {
    pub fn is_claim_lost(&self) -> bool {
        matches!(self, Self::ClaimLost { .. })
    }
}

impl From<refinery::Error> for Error {
    fn from(error: refinery::Error) -> Self {
        Self::Migrate(error.to_string())
    }
}
