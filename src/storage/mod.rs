pub mod db;
pub mod oplog;
pub mod git_interop;

use anyhow::Result;
use std::path::Path;
use colored::*;

pub use db::Database;
pub use oplog::OperationLog;

const FORGE_DIR: &str = ".dx/forge";

pub async fn init(path: &Path) -> Result<()> {
    let forge_path = path.join(FORGE_DIR);

    tokio::fs::create_dir_all(&forge_path).await?;
    tokio::fs::create_dir_all(forge_path.join("objects")).await?;
    tokio::fs::create_dir_all(forge_path.join("refs")).await?;
    tokio::fs::create_dir_all(forge_path.join("logs")).await?;
    tokio::fs::create_dir_all(forge_path.join("context")).await?;

    // Initialize database
    let db = Database::new(&forge_path)?;
    db.initialize()?;

    // Create config
    let config = serde_json::json!({
        "version": "0.1.0",
        "actor_id": uuid::Uuid::new_v4().to_string(),
        "git_interop": true,
        "real_time_sync": false,
    });

    tokio::fs::write(
        forge_path.join("config.json"),
        serde_json::to_string_pretty(&config)?
    ).await?;

    Ok(())
}

pub async fn show_log(file: Option<std::path::PathBuf>, limit: usize) -> Result<()> {
    let db = Database::open(".dx/forge")?;
    let operations = db.get_operations(file.as_deref(), limit)?;

    println!("{}", "Operation Log".cyan().bold());
    println!("{}", "═".repeat(80).bright_black());

    for op in operations {
        let time = op.timestamp.format("%Y-%m-%d %H:%M:%S%.3f");
        let op_type = match &op.op_type {
            crate::crdt::OperationType::Insert { length, .. } =>
                format!("+{} chars", length).green(),
            crate::crdt::OperationType::Delete { length, .. } =>
                format!("-{} chars", length).red(),
            crate::crdt::OperationType::Replace { old_content, new_content, .. } =>
                format!("~{}->{} chars", old_content.len(), new_content.len()).yellow(),
            crate::crdt::OperationType::FileCreate { .. } =>
                "FILE_CREATE".bright_green(),
            crate::crdt::OperationType::FileDelete =>
                "FILE_DELETE".bright_red(),
            crate::crdt::OperationType::FileRename { old_path, new_path } =>
                format!("RENAME {} -> {}", old_path, new_path).bright_yellow(),
        };

        println!(
            "{} {} {} {}",
            format!("[{}]", time).bright_black(),
            op_type.bold(),
            op.file_path.bright_white(),
            format!("({})", op.id).bright_black()
        );
    }

    Ok(())
}

pub async fn git_sync(path: &Path) -> Result<()> {
    git_interop::sync_with_git(path).await
}

pub async fn time_travel(file: &Path, timestamp: Option<String>) -> Result<()> {
    println!("{}", format!("🕐 Time traveling: {}", file.display()).cyan().bold());

    let db = Database::open(".dx/forge")?;
    let operations = db.get_operations(Some(file), 1000)?;

    // Reconstruct file state at timestamp
    let target_time = if let Some(ts) = timestamp {
        chrono::DateTime::parse_from_rfc3339(&ts)?.with_timezone(&chrono::Utc)
    } else {
        chrono::Utc::now()
    };

    let mut content = String::new();

    for op in operations.iter().filter(|o| o.timestamp <= target_time) {
        // Apply operations chronologically
        match &op.op_type {
            crate::crdt::OperationType::FileCreate { content: c } => {
                content = c.clone();
            }
            // ... other operations
            _ => {}
        }
    }

    println!("\n{}", "─".repeat(80).bright_black());
    println!("{}", content);
    println!("{}", "─".repeat(80).bright_black());

    Ok(())
}