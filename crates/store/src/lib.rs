//! Postgres storage for objects, definitions and the change log.
//!
//! Object values are JSONB documents in `object_values`. [`STORAGE_FORM`] decides
//! how those documents are split. Spike 1 measured every form; see
//! `docs/measurements/2026-10-24-spike-1-storage.md`.

mod error;
mod ids;
mod ops;
mod session;
mod split;
pub mod testing;
mod types;

pub use error::Error;
pub use ids::{Actor, BatchId, BatchKind, ChangeOp, PrincipalId, PrincipalKind};
pub use session::Store;
pub use split::StorageForm;

/// Connections used to create the databases the spike and the tests measure.
pub mod admin {
    pub use crate::session::{PgConn, create_database, drop_database, parse_config};
}
pub use types::{BulkReport, BulkRowError, ChangeRow, ObjectOp, StoredObject, WriteReceipt};

/// Form used by [`Store::connect`].
///
/// Spike 1 picks this from measured WAL and write rate. Until that measurement
/// is in `docs/measurements/`, the value is only a placeholder.
/// Chosen by spike 1: edits rewrite one small part, and import was fastest too.
pub const STORAGE_FORM: StorageForm = StorageForm::PerLanguageGroup;
