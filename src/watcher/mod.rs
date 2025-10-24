pub mod detector;

use anyhow::Result;
use std::path::PathBuf;
use colored::*;

use crate::storage::{Database, OperationLog};
use crate::crdt::{CrdtDocument, Operation, OperationType, Position};

pub async fn watch(path: PathBuf, enable_sync: bool) -> Result<()> {
    println!("{}", "Initializing operation tracker...".bright_cyan());

    let db = Database::open(".dx/forge")?;
    let oplog = std::sync::Arc::new(OperationLog::new(std::sync::Arc::new(db)));

    // Load config
    let config: serde_json::Value = serde_json::from_str(
        &tokio::fs::read_to_string(".dx/forge/config.json").await?
    )?;
    let actor_id = config["actor_id"].as_str().unwrap().to_string();

    println!("{} Actor ID: {}", "→".bright_blue(), actor_id.bright_yellow());
    println!("{} Sync: {}\n", "→".bright_blue(), if enable_sync { "enabled".green() } else { "disabled".red() });

    detector::start_watching(path, oplog, actor_id).await?;

    Ok(())
}