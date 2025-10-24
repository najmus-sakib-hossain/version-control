use anyhow::Result;
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
        // Store in database
        let inserted = self.db.store_operation(&operation)?;

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
