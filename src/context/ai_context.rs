use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIContext {
    pub operation_id: Uuid,
    pub reasoning: String,
    pub assumptions: Vec<String>,
    pub constraints: Vec<String>,
    pub related_discussions: Vec<Uuid>,
}

impl AIContext {
    pub fn new(operation_id: Uuid, reasoning: String) -> Self {
        Self {
            operation_id,
            reasoning,
            assumptions: Vec::new(),
            constraints: Vec::new(),
            related_discussions: Vec::new(),
        }
    }
}