pub mod detector;

use anyhow::Result;
use std::path::PathBuf;
use colored::*;

use crate::storage::{Database, OperationLog};
use crate::crdt::{CrdtDocument, Operation, OperationType, Position};
use crate::sync::{SyncManager, remote::connect_peer};
use std::sync::Arc as StdArc;

pub async fn watch(path: PathBuf, enable_sync: bool, peers: Vec<String>) -> Result<()> {
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

    let sync_mgr = if enable_sync { Some(StdArc::new(SyncManager::new())) } else { None };

    // If remote peers provided, connect and bridge
    if let (Some(mgr), true) = (&sync_mgr, !peers.is_empty()) {
        for url in peers {
            let _ = connect_peer(&url, actor_id.clone(), mgr.as_ref().clone(), oplog.clone()).await;
            println!("{} Connected peer {}", "↔".bright_blue(), url.bright_yellow());
        }
    }

    detector::start_watching(path, oplog, actor_id, sync_mgr).await?;

    Ok(())
}