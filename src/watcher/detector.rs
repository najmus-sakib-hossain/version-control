use anyhow::Result;
use notify::{Event, EventKind, RecursiveMode, Watcher};
use notify_debouncer_full::new_debouncer;
use std::path::PathBuf;
use std::time::Duration;
use crossbeam::channel::bounded;
use colored::*;
use std::sync::Arc;

use crate::storage::OperationLog;
use crate::crdt::{Operation, OperationType, Position};
use similar::TextDiff;

pub async fn start_watching(
    path: PathBuf,
    oplog: Arc<OperationLog>,
    actor_id: String,
) -> Result<()> {
    let (tx, rx) = bounded::<notify::Event>(10000);

    let mut debouncer = new_debouncer(
        Duration::from_millis(50),
        None,
        move |result: Result<Vec<notify_debouncer_full::DebouncedEvent>, Vec<notify::Error>>| {
            if let Ok(events) = result {
                for event in events {
                    let _ = tx.send(event.event);
                }
            }
        },
    )?;

    debouncer.watch(&path, RecursiveMode::Recursive)?;

    println!("{}", "👁  Watching for operations...".bright_cyan().bold());

    while let Ok(event) = rx.recv() {
        let start = std::time::Instant::now();

        match event.kind {
            EventKind::Modify(_) => {
                for path in &event.paths {
                    if should_track(path) {
                        if let Ok(op) = detect_operations(path, &actor_id).await {
                            oplog.append(op.clone()).await?;

                            let elapsed = start.elapsed().as_micros();
                            print_operation(&op, elapsed);
                        }
                    }
                }
            }
            EventKind::Create(_) => {
                for path in &event.paths {
                    if should_track(path) && path.is_file() {
                        let content = tokio::fs::read_to_string(path).await.unwrap_or_default();
                        let op = Operation::new(
                            path.display().to_string(),
                            OperationType::FileCreate { content },
                            actor_id.clone(),
                        );

                        oplog.append(op.clone()).await?;

                        let elapsed = start.elapsed().as_micros();
                        print_operation(&op, elapsed);
                    }
                }
            }
            EventKind::Remove(_) => {
                for path in &event.paths {
                    if should_track(path) {
                        let op = Operation::new(
                            path.display().to_string(),
                            OperationType::FileDelete,
                            actor_id.clone(),
                        );

                        oplog.append(op.clone()).await?;

                        let elapsed = start.elapsed().as_micros();
                        print_operation(&op, elapsed);
                    }
                }
            }
            _ => {}
        }
    }

    Ok(())
}

async fn detect_operations(path: &PathBuf, actor_id: &str) -> Result<Operation> {
    // Read current content
    let new_content = tokio::fs::read_to_string(path).await?;

    // For simplicity, treat as full file update
    // In production, would do character-level diffing
    let op = Operation::new(
        path.display().to_string(),
        OperationType::FileCreate { content: new_content },
        actor_id.to_string(),
    );

    Ok(op)
}

fn should_track(path: &PathBuf) -> bool {
    let path_str = path.to_string_lossy();
    !path_str.contains("/.git/")
        && !path_str.contains("/.dx/")
        && !path_str.contains("/target/")
        && !path_str.contains("/node_modules/")
}

fn print_operation(op: &Operation, micros: u128) {
    let time = format!("[{}µs]", micros);
    let time_colored = if micros < 100 {
        time.bright_green()
    } else if micros < 500 {
        time.yellow()
    } else {
        time.red()
    };

    let (action, details) = match &op.op_type {
        OperationType::Insert { length, .. } =>
            ("INSERT".green(), format!("+{} chars", length)),
        OperationType::Delete { length, .. } =>
            ("DELETE".red(), format!("-{} chars", length)),
        OperationType::Replace { old_content, new_content, .. } =>
            ("REPLACE".yellow(), format!("{}→{} chars", old_content.len(), new_content.len())),
        OperationType::FileCreate { .. } =>
            ("CREATE".bright_green(), "file".to_string()),
        OperationType::FileDelete =>
            ("DELETE".bright_red(), "file".to_string()),
        OperationType::FileRename { old_path, new_path } =>
            ("RENAME".bright_yellow(), format!("{} → {}", old_path, new_path)),
    };

    println!(
        "{} {} {} {} {}",
        time_colored,
        action.bold(),
        op.file_path.bright_white(),
        details.bright_black(),
        format!("({})", op.id).bright_black()
    );
}