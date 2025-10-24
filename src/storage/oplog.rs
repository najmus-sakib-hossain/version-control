use anyhow::{Result, anyhow};
use dashmap::DashMap;
use std::sync::Arc;
use uuid::Uuid;

use super::Database;
use crate::crdt::Operation;

pub struct OperationLog {
    db: Arc<Database>,
    // In-memory cache for fast lookups
    cache: DashMap<Uuid, Operation>,
}

impl OperationLog {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            cache: DashMap::new(),
        }
    }

    pub async fn append(&self, operation: Operation) -> Result<bool> {
        let db = self.db.clone();
        let op_for_db = operation.clone();

        let inserted = tokio::task::spawn_blocking(move || db.store_operation(&op_for_db))
            .await
            .map_err(|err| anyhow!("failed to join database task: {err}"))??;

        if inserted {
            // Cache for fast access
            self.cache.insert(operation.id, operation);
        }

        Ok(inserted)
    }

    #[allow(dead_code)]
    pub fn get(&self, id: &Uuid) -> Option<Operation> {
        self.cache.get(id).map(|op| op.clone())
    }
}
