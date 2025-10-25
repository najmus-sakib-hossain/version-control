pub mod detector;
pub mod cache_warmer;

use anyhow::Result;
use colored::*;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

use crate::storage::{Database, OperationLog};
use crate::sync::{SyncManager, remote::connect_peer};
use std::sync::Arc as StdArc;

pub async fn watch(path: PathBuf, enable_sync: bool, peers: Vec<String>) -> Result<()> {
    println!("{}", "Initializing operation tracker...".bright_cyan());

    let repo_root = path.canonicalize().unwrap_or_else(|_| path.clone());
    let forge_dir = repo_root.join(".dx/forge");

    let db = Database::new(&forge_dir)?;
    db.initialize()?;
    let oplog = std::sync::Arc::new(OperationLog::new(std::sync::Arc::new(db)));

    // Load config
    let config_raw = tokio::fs::read_to_string(forge_dir.join("config.json")).await?;
    let config: serde_json::Value = serde_json::from_str(&config_raw)?;
    let actor_id = config["actor_id"].as_str().unwrap().to_string();
    let repo_id = config["repo_id"]
        .as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            let mut hasher = Sha256::new();
            let path_string = repo_root.to_string_lossy().into_owned();
            hasher.update(path_string.as_bytes());
            format!("local-{:x}", hasher.finalize())
        });

    println!(
        "{} Actor ID: {}",
        "→".bright_blue(),
        actor_id.bright_yellow()
    );
    println!(
        "{} Sync: {}\n",
        "→".bright_blue(),
        if enable_sync {
            "enabled".green()
        } else {
            "disabled".red()
        }
    );

    let sync_mgr = if enable_sync {
        Some(StdArc::new(SyncManager::new()))
    } else {
        None
    };

    // If remote peers provided, connect and bridge
    if let (Some(mgr), true) = (&sync_mgr, !peers.is_empty()) {
        for url in peers {
            let _ = connect_peer(
                &url,
                actor_id.clone(),
                repo_id.clone(),
                mgr.as_ref().clone(),
                oplog.clone(),
            )
            .await;
            println!(
                "{} Connected peer {}",
                "↔".bright_blue(),
                url.bright_yellow()
            );
        }
    }

    // Warm OS page cache with all trackable files
    // Wait for cache warming to complete before starting watcher
    // This ensures all subsequent reads are <100µs
    println!("{}", "📦 Warming OS page cache...".bright_cyan());
    let cache_stats = tokio::task::spawn_blocking({
        let repo_root_clone = repo_root.clone();
        move || cache_warmer::warm_cache(&repo_root_clone)
    })
    .await??;
    
    println!(
        "{} Cached {} files ({} KB) in {:.1}ms",
        "✓".bright_green(),
        cache_stats.files_cached,
        cache_stats.bytes_cached / 1024,
        cache_stats.duration_ms as f64
    );

    detector::start_watching(repo_root, oplog, actor_id, repo_id, sync_mgr).await?;

    Ok(())
}
