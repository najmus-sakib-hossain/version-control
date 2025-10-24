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
use crate::sync::SyncManager;
use std::sync::Arc as StdArc;
use similar::TextDiff;

pub async fn start_watching(
    path: PathBuf,
    oplog: Arc<OperationLog>,
    actor_id: String,
    sync_mgr: Option<StdArc<SyncManager>>,
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
                        if let Ok(ops) = detect_operations(path, &actor_id).await {
                            let elapsed = start.elapsed().as_micros();
                            for op in ops.iter() {
                                oplog.append(op.clone()).await?;
                                if let Some(mgr) = &sync_mgr {
                                    let _ = mgr.publish(StdArc::new(op.clone()));
                                }
                                print_operation(op, elapsed);
                            }
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

                        if let Some(mgr) = &sync_mgr {
                            let _ = mgr.publish(StdArc::new(op.clone()));
                        }

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

                        if let Some(mgr) = &sync_mgr {
                            let _ = mgr.publish(StdArc::new(op.clone()));
                        }

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

use once_cell::sync::Lazy;
use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};

static PREV_CONTENT: Lazy<DashMap<PathBuf, String>> = Lazy::new(|| DashMap::new());
static LAMPORT: AtomicU64 = AtomicU64::new(1);

async fn detect_operations(path: &PathBuf, actor_id: &str) -> Result<Vec<Operation>> {
    let new_content = tokio::fs::read_to_string(path).await.unwrap_or_default();
    let key = path.clone();
    let prev_opt = PREV_CONTENT.get(&key).map(|r| r.clone());

    // If no previous content, treat as create
    if prev_opt.is_none() {
        PREV_CONTENT.insert(key, new_content.clone());
        return Ok(vec![Operation::new(
            path.display().to_string(),
            OperationType::FileCreate { content: new_content },
            actor_id.to_string(),
        )]);
    }

    let old_content = prev_opt.unwrap();

    let diff = TextDiff::from_chars(&old_content, &new_content);

    // Cursor to compute line/col and offsets
    let mut old_idx: usize = 0;
    let mut new_idx: usize = 0;
    let mut ops: Vec<Operation> = Vec::new();

    for change in diff.ops() {
        use similar::DiffOp;
        match change {
            DiffOp::Equal { len, .. } => {
                // advance both cursors
                old_idx += len;
                new_idx += len;
            }
            DiffOp::Insert { new_len, .. } => {
                let inserted: String = new_content.chars().skip(new_idx).take(*new_len).collect();
                let (line, col) = offset_to_line_col(&new_content, new_idx);
                let lamport = LAMPORT.fetch_add(1, Ordering::Relaxed);
                let pos = Position::new(line, col, new_idx, actor_id.to_string(), lamport);
                ops.push(Operation::new(
                    path.display().to_string(),
                    OperationType::Insert { position: pos, content: inserted, length: *new_len },
                    actor_id.to_string(),
                ));
                new_idx += *new_len;
            }
            DiffOp::Delete { old_len, .. } => {
                let deleted: String = old_content.chars().skip(old_idx).take(*old_len).collect();
                let (line, col) = offset_to_line_col(&old_content, old_idx);
                let lamport = LAMPORT.fetch_add(1, Ordering::Relaxed);
                let pos = Position::new(line, col, old_idx, actor_id.to_string(), lamport);
                ops.push(Operation::new(
                    path.display().to_string(),
                    OperationType::Delete { position: pos, length: *old_len },
                    actor_id.to_string(),
                ));
                old_idx += *old_len;
            }
            DiffOp::Replace { old_len, new_len, .. } => {
                let deleted: String = old_content.chars().skip(old_idx).take(*old_len).collect();
                let inserted: String = new_content.chars().skip(new_idx).take(*new_len).collect();
                let (line, col) = offset_to_line_col(&old_content, old_idx);
                let lamport = LAMPORT.fetch_add(1, Ordering::Relaxed);
                let pos = Position::new(line, col, old_idx, actor_id.to_string(), lamport);
                ops.push(Operation::new(
                    path.display().to_string(),
                    OperationType::Replace { position: pos, old_content: deleted, new_content: inserted },
                    actor_id.to_string(),
                ));
                old_idx += *old_len;
                new_idx += *new_len;
            }
        }
    }

    PREV_CONTENT.insert(key, new_content);
    Ok(ops)
}

fn offset_to_line_col(s: &str, offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for (i, ch) in s.chars().enumerate() {
        if i == offset { break; }
        if ch == '\n' { line += 1; col = 1; } else { col += 1; }
    }
    (line, col)
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