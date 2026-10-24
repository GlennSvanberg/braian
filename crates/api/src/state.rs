use std::sync::Arc;

use openpim_model::DefinitionSet;
use openpim_rules::CompiledModel;
use openpim_store::{Error, Store};
use tokio::sync::RwLock;

use crate::auth::Identity;
use crate::problem::{Problem, store_status};

#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub identities: Arc<Vec<Identity>>,
    pub models: ModelCache,
}

pub struct CachedModel {
    pub revision: u64,
    pub defs: DefinitionSet,
    pub model: CompiledModel,
}

#[derive(Clone, Default)]
pub struct ModelCache {
    inner: Arc<RwLock<Option<Arc<CachedModel>>>>,
}

impl ModelCache {
    pub async fn get(&self, store: &Store) -> Result<Arc<CachedModel>, Problem> {
        let Some(revision) = store.model_revision().await.map_err(store_status)? else {
            return Err(store_status(Error::NoDefinitions));
        };
        if let Some(cached) = self.inner.read().await.clone()
            && cached.revision == revision
        {
            return Ok(cached);
        }
        let defs = store.definition_set().await.map_err(store_status)?;
        let model = CompiledModel::compile(&defs).map_err(Problem::definitions)?;
        let cached = Arc::new(CachedModel {
            revision: defs.model_revision,
            defs,
            model,
        });
        *self.inner.write().await = Some(Arc::clone(&cached));
        Ok(cached)
    }

    pub async fn invalidate(&self) {
        *self.inner.write().await = None;
    }
}
