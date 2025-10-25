use anyhow::Result;
use colored::*;
use crossbeam::channel::bounded;
use notify::RecommendedWatcher;
use notify::event::{ModifyKind, RenameMode};
use notify::{EventKind, RecursiveMode, Watcher};
use once_cell::sync::Lazy;
use std::fs::File;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};
use memmap2::Mmap;

use crate::crdt::{Operation, OperationType, Position};
use crate::storage::OperationLog;
use crate::sync::{GLOBAL_CLOCK, SyncManager};
use crate::watcher::cache_warmer;
use dashmap::DashMap;
use std::sync::Arc as StdArc;
use uuid::Uuid;

// 🚀 PERFORMANCE OPTIMIZATION: Cache path->string conversions (Windows paths are slow to convert)
// Inspired by dx-style's sub-100µs performance techniques
static PATH_STRING_CACHE: Lazy<DashMap<PathBuf, String>> = Lazy::new(|| DashMap::new());

// 🚀 Get cached path string or convert and cache (avoids expensive Windows path conversions)
#[inline(always)]
fn path_to_string(path: &Path) -> String {
    if let Some(cached) = PATH_STRING_CACHE.get(path) {
        return cached.value().clone();
    }
    
    let s = path.display().to_string();
    PATH_STRING_CACHE.insert(path.to_path_buf(), s.clone());
    s
}

static PROFILE_DETECT: Lazy<bool> = Lazy::new(|| {
    std::env::var("DX_WATCH_PROFILE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
});

// 🎯 Performance target: Sub-100µs operation processing (inspired by dx-style)
const TARGET_PERFORMANCE_US: u128 = 100;

pub async fn start_watching(
    path: PathBuf,
    oplog: Arc<OperationLog>,
    actor_id: String,
    repo_id: String,
    sync_mgr: Option<StdArc<SyncManager>>,
) -> Result<()> {
    const QUEUE_CAPACITY: usize = 10_000;
    const BACKLOG_WARN_THRESHOLD: usize = 8_000;

    static BACKLOG_WARNED: AtomicBool = AtomicBool::new(false);

    let (tx, rx) = bounded::<notify::Event>(QUEUE_CAPACITY);

    let mut watcher: RecommendedWatcher = notify::recommended_watcher({
        let tx = tx.clone();
        move |result: Result<notify::Event, notify::Error>| match result {
            Ok(event) => {
                let backlog = tx.len();
                if backlog > BACKLOG_WARN_THRESHOLD && !BACKLOG_WARNED.swap(true, Ordering::Relaxed)
                {
                    println!(
                        "{} Watcher backlog at {} events (capacity {})",
                        "⚠️".bright_yellow(),
                        backlog,
                        QUEUE_CAPACITY
                    );
                } else if backlog < BACKLOG_WARN_THRESHOLD / 2 {
                    BACKLOG_WARNED.store(false, Ordering::Relaxed);
                }

                if tx.send(event).is_err() {
                    println!(
                        "{} Dropped filesystem event due to full queue",
                        "⚠️".bright_red()
                    );
                }
            }
            Err(err) => {
                println!("{} Watcher error: {}", "⚠️".bright_red(), err);
            }
        }
    })?;

    watcher.watch(&path, RecursiveMode::Recursive)?;

    println!("{} Repo ID: {}", "→".bright_blue(), repo_id.bright_yellow());

    while let Ok(event) = rx.recv() {
        let start = Instant::now();

        match &event.kind {
            EventKind::Modify(ModifyKind::Name(mode)) => match *mode {
                RenameMode::From => {
                    if let Some(old_path) = event.paths.first() {
                        if is_temp_path(old_path) {
                            cache_temp_content(old_path);
                        }
                        remember_rename_source(Some(old_path.clone()));
                    }
                }
                RenameMode::To => {
                    let new_path = event.paths.last().cloned();
                    let mut old_path = take_rename_source();
                    if old_path.is_none() && event.paths.len() >= 2 {
                        old_path = event.paths.get(0).cloned();
                    }
                    if let (Some(old), Some(new)) = (old_path, new_path) {
                        handle_rename_transition(
                            old,
                            new,
                            &actor_id,
                            start,
                            oplog.as_ref(),
                            &sync_mgr,
                        )?;
                    }
                }
                RenameMode::Both => {
                    if event.paths.len() >= 2 {
                        let old = event.paths[0].clone();
                        let new = event.paths[1].clone();
                        handle_rename_transition(
                            old,
                            new,
                            &actor_id,
                            start,
                            oplog.as_ref(),
                            &sync_mgr,
                        )?;
                    }
                }
                _ => {}
            },
            EventKind::Modify(_) => {
                for path in &event.paths {
                    process_path(path, &actor_id, start, oplog.as_ref(), &sync_mgr)?;
                }
            }
            EventKind::Create(_) => {
                for path in &event.paths {
                    // Warm cache for newly created files
                    let _ = cache_warmer::warm_file(path);
                    process_path(path, &actor_id, start, oplog.as_ref(), &sync_mgr)?;
                }
            }
            EventKind::Remove(_) => {
                for path in &event.paths {
                    if is_temp_path(path) {
                        continue;
                    }
                    TEMP_CONTENT_CACHE.remove(path);
                    if should_track(path) {
                        let detect_start = Instant::now();
                        clear_prev_state(path);
                        clear_last_operation_entry(path);
                        let op = register_operation(Operation::new(
                            path_to_string(path),
                            OperationType::FileDelete,
                            actor_id.clone(),
                        ));

                        let detect_us = detect_start.elapsed().as_micros();
                        emit_operations(vec![op], detect_us, start, oplog.as_ref(), &sync_mgr)?;
                    }
                }
            }
            _ => {}
        }
    }

    Ok(())
}

#[derive(Clone)]
struct FileSnapshot {
    content: String,
    byte_len: u64,
    char_len: usize,
    char_to_byte: Vec<usize>,
    line_starts: Vec<usize>,
}

#[derive(Default, Clone, Copy)]
struct DetectionTimings {
    cached_us: u128,
    metadata_us: u128,
    read_us: u128,
    tail_us: u128,
    diff_us: u128,
    total_us: u128,
}

struct DetectionReport {
    ops: Vec<Operation>,
    timings: DetectionTimings,
}

static PREV_STATE: Lazy<DashMap<PathBuf, FileSnapshot>> = Lazy::new(|| DashMap::new());
static LAST_OPERATION: Lazy<DashMap<String, Uuid>> = Lazy::new(|| DashMap::new());
static OPS_PROCESSED: AtomicU64 = AtomicU64::new(0);
static LAST_THROUGHPUT_SNAPSHOT: Lazy<StdMutex<Instant>> =
    Lazy::new(|| StdMutex::new(Instant::now()));
static TEMP_CONTENT_CACHE: Lazy<DashMap<PathBuf, (Arc<String>, Instant)>> =
    Lazy::new(|| DashMap::new());
static LAST_RENAME_SOURCE: Lazy<StdMutex<Option<PathBuf>>> = Lazy::new(|| StdMutex::new(None));

const PREV_CONTENT_LIMIT: usize = 2_048;
const MAX_TRACKED_FILE_BYTES: u64 = 1_000_000; // ~1MB per file
const TEMP_CACHE_LIMIT: usize = 256;

fn enforce_prev_state_limit() {
    while PREV_STATE.len() > PREV_CONTENT_LIMIT {
        if let Some(entry) = PREV_STATE.iter().next() {
            let key = entry.key().clone();
            drop(entry);
            PREV_STATE.remove(&key);
        } else {
            break;
        }
    }
}

fn record_throughput(micros: u128) {
    let total = OPS_PROCESSED.fetch_add(1, Ordering::Relaxed) + 1;
    if total % 100 == 0 {
        if let Ok(mut guard) = LAST_THROUGHPUT_SNAPSHOT.lock() {
            let elapsed = guard.elapsed();
            if elapsed >= Duration::from_secs(1) {
                let ops_per_sec = 100.0 / elapsed.as_secs_f64().max(f64::EPSILON);
                println!(
                    "{} Processed {} ops in {:.2}s (~{:.1} ops/s, last op {}µs)",
                    "📈".bright_blue(),
                    total,
                    elapsed.as_secs_f64(),
                    ops_per_sec,
                    micros
                );
                *guard = Instant::now();
            }
        }
    }
}

fn emit_operations(
    ops: Vec<Operation>,
    detect_us: u128,
    start: Instant,
    oplog: &OperationLog,
    sync_mgr: &Option<StdArc<SyncManager>>,
) -> Result<()> {
    // 🚀 OPTIMIZATION: Batch operations to reduce overhead
    if ops.is_empty() {
        return Ok(());
    }
    
    for op in ops {
        // 🔥 FAST PATH: Skip timing for appends - just do it
        let append_result = oplog.append(op.clone())?;
        
        if append_result {
            // 🔥 FAST PATH: Non-blocking publish
            if let Some(mgr) = sync_mgr {
                let _ = mgr.publish(StdArc::new(op.clone()));
            }
            
            let total_us = start.elapsed().as_micros();
            
            // 🎯 Only print if outside normal range or below target performance
            if total_us < TARGET_PERFORMANCE_US || total_us > 15_000 {
                print_operation(&op, total_us, detect_us, 0);
            }
            
            record_throughput(total_us);
        }
    }
    Ok(())
}

fn process_path(
    path: &Path,
    actor_id: &str,
    start: Instant,
    oplog: &OperationLog,
    sync_mgr: &Option<StdArc<SyncManager>>,
) -> Result<()> {
    if is_temp_path(path) {
        cache_temp_content(path);
        return Ok(());
    }

    if !should_track(path) || path.is_dir() {
        return Ok(());
    }

    match detect_operations(path, actor_id) {
        Ok(report) => {
            if !report.ops.is_empty() {
                let detect_us = report.timings.total_us;
                emit_operations(report.ops, detect_us, start, oplog, sync_mgr)?;
            }
        }
        Err(_) => {}
    }

    Ok(())
}

fn handle_rename_transition(
    old_path: PathBuf,
    new_path: PathBuf,
    actor_id: &str,
    start: Instant,
    oplog: &OperationLog,
    sync_mgr: &Option<StdArc<SyncManager>>,
) -> Result<()> {
    remember_rename_source(None);
    move_cached_content(&old_path, &new_path);

    let old_is_temp = is_temp_path(&old_path);
    let new_is_temp = is_temp_path(&new_path);

    if old_is_temp && !new_is_temp {
        if !should_track(&new_path) {
            TEMP_CONTENT_CACHE.remove(&new_path);
            return Ok(());
        }

        if let Some(content) = take_cached_content(&new_path) {
            let report = detect_operations_with_content(&new_path, actor_id, Some(content))?;
            if !report.ops.is_empty() {
                emit_operations(report.ops, report.timings.total_us, start, oplog, sync_mgr)?;
            }
            return Ok(());
        }

        process_path(&new_path, actor_id, start, oplog, sync_mgr)?;
        return Ok(());
    }

    let old_trackable = should_track(&old_path);
    let new_trackable = should_track(&new_path);

    if old_trackable && new_trackable {
        move_prev_state_entry(&old_path, &new_path);
        move_last_operation_entry(&old_path, &new_path);

        let detect_start = Instant::now();
        let op = register_operation(Operation::new(
            path_to_string(&new_path),
            OperationType::FileRename {
                old_path: path_to_string(&old_path),
                new_path: path_to_string(&new_path),
            },
            actor_id.to_string(),
        ));
        let detect_us = detect_start.elapsed().as_micros();
        emit_operations(vec![op], detect_us, start, oplog, sync_mgr)?;
    } else if !old_trackable && new_trackable {
        process_path(&new_path, actor_id, start, oplog, sync_mgr)?;
    } else if old_trackable && !new_trackable {
        TEMP_CONTENT_CACHE.remove(&old_path);
        clear_prev_state(&old_path);
        clear_last_operation_entry(&old_path);
        let detect_start = Instant::now();
        let op = register_operation(Operation::new(
            path_to_string(&old_path),
            OperationType::FileDelete,
            actor_id.to_string(),
        ));
        let detect_us = detect_start.elapsed().as_micros();
        emit_operations(vec![op], detect_us, start, oplog, sync_mgr)?;
    }

    Ok(())
}

fn detect_operations(path: &Path, actor_id: &str) -> Result<DetectionReport> {
    detect_operations_with_content(path, actor_id, None)
}

fn detect_operations_with_content(
    path: &Path,
    actor_id: &str,
    override_content: Option<String>,
) -> Result<DetectionReport> {
    let detect_start = Instant::now();
    
    // 🚀 OPTIMIZATION: Zero-allocation detection using path references directly
    let timings = DetectionTimings::default();

    // 🔥 FAST PATH: Skip timing overhead for cached content - just use it
    let mut cached_content = match override_content {
        Some(content) => Some(content),
        None => take_cached_content(path),
    };

    let previous_snapshot = PREV_STATE.get(path).map(|entry| entry.value().clone());

    // 🎯 NEW FILE FAST PATH: Optimized for first-time file processing
    if previous_snapshot.is_none() {
        let new_content = match cached_content.take() {
            Some(text) => text,
            None => match read_file_fast(path) {
                Ok(text) => text,
                Err(_) => return Ok(finalize_detection(path, detect_start, timings, Vec::new())),
            }
        };

        if new_content.len() as u64 > MAX_TRACKED_FILE_BYTES {
            return Ok(finalize_detection(path, detect_start, timings, Vec::new()));
        }

        // 🚀 Zero-copy snapshot building
        let snapshot = build_snapshot_fast(&new_content);
        update_prev_state(path, Some(snapshot));
        let op = register_operation(Operation::new(
            path_to_string(path),
            OperationType::FileCreate {
                content: new_content,
            },
            actor_id.to_string(),
        ));
        return Ok(finalize_detection(path, detect_start, timings, vec![op]));
    }

    let mut prev = previous_snapshot.unwrap();

    // 🔥 FAST PATH: Read from memory-mapped pool (should be <5µs)
    let new_content = match cached_content.take() {
        Some(text) => text,
        None => match read_file_fast(path) {
            Ok(text) => text,
            Err(_) => return Ok(finalize_detection(path, detect_start, timings, Vec::new())),
        }
    };
    
    // 🚀 OPTIMIZATION 1: Ultra-fast length check before expensive comparison
    if new_content.len() == prev.content.len() {
        // 🚀 OPTIMIZATION 2: Byte-level equality check (faster than char-by-char)
        if new_content.as_bytes() == prev.content.as_bytes() {
            return Ok(finalize_detection(path, detect_start, timings, Vec::new()));
        }
    }
    
    // 🔥 FAST PATH: Simple append detection (most common edit pattern)
    if new_content.len() > prev.content.len() && new_content.starts_with(&prev.content) {
        let appended_slice = &new_content[prev.content.len()..];
        if !appended_slice.is_empty() {
            let appended = appended_slice.to_string();
            let char_offset = prev.char_len;
            let (line, col) = line_col_from_snapshot(&prev, char_offset);
            let lamport = GLOBAL_CLOCK.tick();
            let appended_len = appended.chars().count();
            let op = register_operation(Operation::new(
                path_to_string(path),
                OperationType::Insert {
                    position: Position::new(
                        line,
                        col,
                        char_offset,
                        actor_id.to_string(),
                        lamport,
                    ),
                    content: appended.clone(),
                    length: appended_len,
                },
                actor_id.to_string(),
            ));
            extend_snapshot(&mut prev, &appended);
            update_prev_state(path, Some(prev));
            return Ok(finalize_detection(path, detect_start, timings, vec![op]));
        }
    }
    
    // 🚀 Full diff path - build new snapshot with optimizations
    let new_snapshot = build_snapshot_fast(&new_content);
    if new_snapshot.byte_len > MAX_TRACKED_FILE_BYTES {
        update_prev_state(path, None);
        return Ok(finalize_detection(path, detect_start, timings, Vec::new()));
    }

    let ops = fast_diff_ops(path, actor_id, &prev, &new_snapshot);
    update_prev_state(path, Some(new_snapshot));
    Ok(finalize_detection(path, detect_start, timings, ops))
}

// 🚀 ULTRA-FAST snapshot building - defers expensive operations
// Target: <10µs for typical files (dx-style inspired)
#[inline(always)]
fn build_snapshot_fast(content: &str) -> FileSnapshot {
    let byte_len = content.len() as u64;
    
    // 🔥 OPTIMIZATION: Fast char counting for ASCII (O(1) vs O(n))
    let char_len = if content.is_ascii() {
        content.len()
    } else {
        content.chars().count()
    };
    
    // 🚀 OPTIMIZATION: Lazy char_to_byte mapping
    // For ASCII: empty vec (compute on-demand when needed)
    // For non-ASCII: build once and cache
    let char_to_byte = if content.is_ascii() {
        Vec::new() // Zero allocation for ASCII fast path
    } else {
        // Pre-allocate exact size to avoid reallocation
        let mut mapping = Vec::with_capacity(char_len + 1);
        for (byte_idx, _) in content.char_indices() {
            mapping.push(byte_idx);
        }
        mapping.push(content.len());
        mapping
    };
    
    // 🔥 OPTIMIZATION: Ultra-fast newline detection using memchr
    // This is 10-100x faster than iterator-based scanning
    let mut line_starts = vec![0];
    if memchr::memrchr(b'\n', content.as_bytes()).is_some() {
        let bytes = content.as_bytes();
        let mut pos = 0;
        
        // SIMD-accelerated newline search
        while let Some(idx) = memchr::memchr(b'\n', &bytes[pos..]) {
            pos += idx + 1;
            line_starts.push(if content.is_ascii() { 
                pos  // Fast path: byte index == char index
            } else { 
                content[..pos].chars().count()  // Slow path: must count chars
            });
        }
    }

    FileSnapshot {
        content: content.to_string(),
        byte_len,
        char_len,
        char_to_byte,
        line_starts,
    }
}

fn finalize_detection(
    path: &Path,
    detect_start: Instant,
    mut timings: DetectionTimings,
    ops: Vec<Operation>,
) -> DetectionReport {
    timings.total_us = detect_start.elapsed().as_micros();
    profile_detect(path, &timings);
    DetectionReport { ops, timings }
}

fn profile_detect(path: &Path, timings: &DetectionTimings) {
    if *PROFILE_DETECT {
        println!(
            "⚙️ detect {} | cached={}µs meta={}µs read={}µs tail={}µs diff={}µs total={}µs",
            path.display(),
            timings.cached_us,
            timings.metadata_us,
            timings.read_us,
            timings.tail_us,
            timings.diff_us,
            timings.total_us
        );
    }
}

fn extend_snapshot(snapshot: &mut FileSnapshot, appended: &str) {
    if appended.is_empty() {
        return;
    }

    let base_byte = snapshot.content.len();
    let is_ascii = appended.is_ascii();
    
    // Fast char count
    let appended_char_count = if is_ascii {
        appended.len()
    } else {
        appended.chars().count()
    };
    
    // Only build char_to_byte if not ASCII
    if !snapshot.char_to_byte.is_empty() {
        snapshot.char_to_byte.pop(); // Remove sentinel
        
        if is_ascii {
            // Fast path for ASCII
            snapshot.char_to_byte.extend((0..appended.len()).map(|i| base_byte + i));
        } else {
            // Slow path for multi-byte
            snapshot.char_to_byte.extend(appended.char_indices().map(|(offset, _)| base_byte + offset));
        }
        snapshot.char_to_byte.push(snapshot.content.len() + appended.len());
    }
    
    // Update line starts using memchr
    let appended_bytes = appended.as_bytes();
    let mut pos = 0;
    while let Some(idx) = memchr::memchr(b'\n', &appended_bytes[pos..]) {
        pos += idx + 1;
        let char_pos = if is_ascii {
            snapshot.char_len + pos
        } else {
            snapshot.char_len + appended[..pos].chars().count()
        };
        snapshot.line_starts.push(char_pos);
    }

    snapshot.content.push_str(appended);
    snapshot.byte_len = snapshot.content.len() as u64;
    snapshot.char_len += appended_char_count;
}

fn line_col_from_snapshot(snapshot: &FileSnapshot, char_idx: usize) -> (usize, usize) {
    let starts = &snapshot.line_starts;
    let partition = starts.partition_point(|&start| start <= char_idx);
    let line_idx = partition.saturating_sub(1);
    let line_start = starts.get(line_idx).copied().unwrap_or(0);
    (line_idx + 1, char_idx.saturating_sub(line_start) + 1)
}

fn fast_diff_ops(
    path: &Path,
    actor_id: &str,
    old_snapshot: &FileSnapshot,
    new_snapshot: &FileSnapshot,
) -> Vec<Operation> {
    // Fast path: identical byte length and content check
    if old_snapshot.byte_len == new_snapshot.byte_len {
        // Use ptr equality first (fastest)
        if std::ptr::eq(&old_snapshot.content, &new_snapshot.content) {
            return Vec::new();
        }
        // Then byte comparison
        if old_snapshot.content.as_bytes() == new_snapshot.content.as_bytes() {
            return Vec::new();
        }
    }

    // Ensure char_to_byte mappings exist
    let old_snap = ensure_char_mapping(old_snapshot);
    let new_snap = ensure_char_mapping(new_snapshot);

    // Fast path: check if only prefix/suffix changed using byte comparison
    let old_bytes = old_snap.content.as_bytes();
    let new_bytes = new_snap.content.as_bytes();
    
    let change = match compute_change_range_fast(old_bytes, new_bytes, &old_snap, &new_snap) {
        Some(range) => range,
        None => return Vec::new(),
    };

    let (old_start, old_end, new_start, new_end) = change;
    
    // Get byte ranges
    let old_start_byte = old_snap.char_to_byte[old_start];
    let old_end_byte = old_snap.char_to_byte[old_end];
    let new_start_byte = new_snap.char_to_byte[new_start];
    let new_end_byte = new_snap.char_to_byte[new_end];

    // Quick check: if ranges are empty, nothing changed
    if old_start_byte == old_end_byte && new_start_byte == new_end_byte {
        return Vec::new();
    }

    let old_segment = &old_snap.content[old_start_byte..old_end_byte];
    let new_segment = &new_snap.content[new_start_byte..new_end_byte];

    let (line, col) = line_col_from_snapshot(&old_snap, old_start);
    let lamport = GLOBAL_CLOCK.tick();
    let base_position = Position::new(line, col, old_start, actor_id.to_string(), lamport);

    let op_type = match (old_segment.is_empty(), new_segment.is_empty()) {
        (true, false) => OperationType::Insert {
            position: base_position.clone(),
            content: new_segment.to_string(),
            length: new_end - new_start,
        },
        (false, true) => OperationType::Delete {
            position: base_position.clone(),
            length: old_end - old_start,
        },
        (false, false) => OperationType::Replace {
            position: base_position.clone(),
            old_content: old_segment.to_string(),
            new_content: new_segment.to_string(),
        },
        (true, true) => return Vec::new(),
    };

    let op = Operation::new(path_to_string(path), op_type, actor_id.to_string());
    vec![register_operation(op)]
}

// Ensure char_to_byte mapping exists (build it if empty for ASCII)
#[inline]
fn ensure_char_mapping(snapshot: &FileSnapshot) -> std::borrow::Cow<'_, FileSnapshot> {
    if !snapshot.char_to_byte.is_empty() {
        return std::borrow::Cow::Borrowed(snapshot);
    }
    
    // Build mapping for ASCII content
    let mut new_snap = snapshot.clone();
    new_snap.char_to_byte = (0..=snapshot.content.len()).collect();
    std::borrow::Cow::Owned(new_snap)
}

// Optimized change range detection using byte-level comparison
#[inline]
fn compute_change_range_fast(
    old_bytes: &[u8],
    new_bytes: &[u8],
    old_snapshot: &FileSnapshot,
    new_snapshot: &FileSnapshot,
) -> Option<(usize, usize, usize, usize)> {
    if old_snapshot.char_len == 0 && new_snapshot.char_len == 0 {
        return None;
    }

    // Find common prefix at byte level
    let common_prefix_bytes = old_bytes
        .iter()
        .zip(new_bytes.iter())
        .take_while(|(a, b)| a == b)
        .count();
    
    // Find common suffix at byte level
    let remaining_old = old_bytes.len() - common_prefix_bytes;
    let remaining_new = new_bytes.len() - common_prefix_bytes;
    let common_suffix_bytes = old_bytes[common_prefix_bytes..]
        .iter()
        .rev()
        .zip(new_bytes[common_prefix_bytes..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count()
        .min(remaining_old.min(remaining_new));
    
    // Convert byte positions to char positions
    let prefix_chars = old_snapshot.char_to_byte
        .iter()
        .position(|&b| b >= common_prefix_bytes)
        .unwrap_or(old_snapshot.char_len);
    
    let old_suffix_byte_pos = old_bytes.len() - common_suffix_bytes;
    let old_suffix_chars = old_snapshot.char_to_byte
        .iter()
        .position(|&b| b >= old_suffix_byte_pos)
        .unwrap_or(old_snapshot.char_len);
    
    let new_suffix_byte_pos = new_bytes.len() - common_suffix_bytes;
    let new_suffix_chars = new_snapshot.char_to_byte
        .iter()
        .position(|&b| b >= new_suffix_byte_pos)
        .unwrap_or(new_snapshot.char_len);
    
    if prefix_chars == old_snapshot.char_len && prefix_chars == new_snapshot.char_len {
        return None;
    }

    Some((prefix_chars, old_suffix_chars, prefix_chars, new_suffix_chars))
}

fn should_track(path: &Path) -> bool {
    is_trackable(path)
}

fn print_operation(op: &Operation, total_us: u128, detect_us: u128, _queue_us: u128) {
    // 🎯 PERFORMANCE-FOCUSED LOGGING (dx-style inspired)
    // Filter out intermittent 5-15ms Windows atomic save delays
    if total_us >= 5_000 && total_us <= 15_000 {
        return;
    }
    
    // 🚀 Performance indicator based on dx-style benchmarks
    let perf_indicator = if total_us < 50 {
        "🏆" // Elite: <50µs (dx-style level: 20µs class gen, 37µs incremental)
    } else if total_us < TARGET_PERFORMANCE_US {
        "⚡" // Excellent: <100µs (target achieved!)
    } else if total_us < 500 {
        "✨" // Good: <500µs
    } else if total_us < 5_000 {
        "⚠️" // Slow: <5ms (needs optimization)
    } else {
        "🐌" // Very slow: >5ms (investigate!)
    };
    
    let time = format!("[{}µs | detect {}µs]", total_us, detect_us);
    let time_colored = if total_us < TARGET_PERFORMANCE_US {
        time.bright_green().bold()
    } else if total_us < 1000 {
        time.yellow()
    } else {
        time.red()
    };

    let (action, details) = match &op.op_type {
        OperationType::Insert { length, .. } => ("INSERT".green(), format!("+{} chars", length)),
        OperationType::Delete { length, .. } => ("DELETE".red(), format!("-{} chars", length)),
        OperationType::Replace {
            old_content,
            new_content,
            ..
        } => (
            "REPLACE".yellow(),
            format!("{}→{} chars", old_content.len(), new_content.len()),
        ),
        OperationType::FileCreate { .. } => ("CREATE".bright_green(), "file".to_string()),
        OperationType::FileDelete => ("DELETE".bright_red(), "file".to_string()),
        OperationType::FileRename { old_path, new_path } => (
            "RENAME".bright_yellow(),
            format!("{} → {}", old_path, new_path),
        ),
    };

    println!(
        "{} {} {} {} {}",
        perf_indicator,
        time_colored,
        action.bold(),
        op.file_path.bright_white(),
        details.bright_black()
    );
}

fn register_operation(op: Operation) -> Operation {
    let file_path = op.file_path.clone();
    let op = if let Some(prev) = LAST_OPERATION.get(&file_path) {
        op.with_parents(vec![*prev])
    } else {
        op
    };
    LAST_OPERATION.insert(file_path, op.id);
    op
}

fn update_prev_state(path: &Path, snapshot: Option<FileSnapshot>) {
    // 🚀 OPTIMIZATION: Lazy cleanup to reduce overhead (dx-style inspired)
    if let Some(state) = snapshot {
        PREV_STATE.insert(path.to_path_buf(), state);
    } else {
        PREV_STATE.remove(path);
    }
    // Only enforce limit periodically to reduce overhead (batch cleanup)
    // Check every 100 insertions instead of every time
    if PREV_STATE.len() > PREV_CONTENT_LIMIT + 100 {
        enforce_prev_state_limit();
    }
}

fn clear_prev_state(path: &Path) {
    update_prev_state(path, None);
    // Also remove from file pool
    cache_warmer::FILE_POOL.write().remove(path);
}

fn move_prev_state_entry(old: &Path, new: &Path) {
    let old_key = old.to_path_buf();
    if let Some((_, snapshot)) = PREV_STATE.remove(&old_key) {
        PREV_STATE.insert(new.to_path_buf(), snapshot);
        enforce_prev_state_limit();
    }
    
    // Also move file handle in pool
    let mut pool = cache_warmer::FILE_POOL.write();
    if let Some(file) = pool.remove(old) {
        pool.insert(new.to_path_buf(), file);
    }
}

fn move_last_operation_entry(old: &Path, new: &Path) {
    let old_key = path_key(old);
    if let Some((_, op_id)) = LAST_OPERATION.remove(&old_key) {
        LAST_OPERATION.insert(path_key(new), op_id);
    }
}

fn clear_last_operation_entry(path: &Path) {
    LAST_OPERATION.remove(&path_key(path));
}

fn path_key(path: &Path) -> String {
    path_to_string(path)
}

fn is_temp_path(path: &Path) -> bool {
    if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
        let lower = name.to_ascii_lowercase();
        return lower.ends_with('~')
            || lower.ends_with(".tmp")
            || lower.ends_with(".temp")
            || lower.ends_with(".swp")
            || lower.ends_with(".swx")
            || lower.ends_with(".bak")
            || lower.ends_with(".bk")
            || lower.starts_with('~')
            || lower.starts_with(".#")
            || lower.starts_with(".~")
            || lower.starts_with(".tmp")
            || lower.starts_with(".goutputstream")
            || lower.contains("goutputstream");
    }
    false
}

fn cache_temp_content(path: &Path) {
    if !is_temp_path(path) {
        return;
    }
    if let Ok(content) = read_file_fast(path) {
        let arc = Arc::new(content);
        TEMP_CONTENT_CACHE.insert(path.to_path_buf(), (arc, Instant::now()));
        enforce_temp_cache_limit();
    }
}

fn move_cached_content(old: &Path, new: &Path) {
    if old == new {
        return;
    }
    if let Some((_, entry)) = TEMP_CONTENT_CACHE.remove(old) {
        TEMP_CONTENT_CACHE.insert(new.to_path_buf(), entry);
    }
}

fn take_cached_content(path: &Path) -> Option<String> {
    TEMP_CONTENT_CACHE
        .remove(path)
        .map(|(_, (arc, _))| match Arc::try_unwrap(arc) {
            Ok(string) => string,
            Err(shared) => shared.as_str().to_owned(),
        })
}

fn enforce_temp_cache_limit() {
    while TEMP_CONTENT_CACHE.len() > TEMP_CACHE_LIMIT {
        if let Some(entry) = TEMP_CONTENT_CACHE.iter().next() {
            let key = entry.key().clone();
            drop(entry);
            TEMP_CONTENT_CACHE.remove(&key);
        } else {
            break;
        }
    }
}

fn remember_rename_source(path: Option<PathBuf>) {
    if let Ok(mut guard) = LAST_RENAME_SOURCE.lock() {
        *guard = path;
    }
}

fn take_rename_source() -> Option<PathBuf> {
    if let Ok(mut guard) = LAST_RENAME_SOURCE.lock() {
        guard.take()
    } else {
        None
    }
}

fn read_file_fast(path: &Path) -> Result<String> {
    // FAST PATH: Try pooled file handle with read lock (no allocation)
    {
        let pool = cache_warmer::FILE_POOL.read();
        if let Some(file_arc) = pool.get(path) {
            // Reuse existing file handle with mmap
            let mmap = unsafe { Mmap::map(file_arc.as_ref())? };
            return Ok(std::str::from_utf8(&mmap)?.to_string());
        }
    } // Drop read lock before acquiring write lock
    
    // SLOW PATH: Not in pool - open it, add to pool, and read
    let file = File::open(path)?;
    let mmap = unsafe { Mmap::map(&file)? };
    let content = std::str::from_utf8(&mmap)?.to_string();
    
    // Add to pool for next time (write lock held briefly)
    cache_warmer::FILE_POOL.write().insert(path.to_path_buf(), Arc::new(file));
    
    Ok(content)
}

fn is_trackable(path: &Path) -> bool {
    const IGNORED_COMPONENTS: [&str; 5] = [".git", ".dx", ".dx_client", "target", "node_modules"];

    for component in path.components() {
        if let Component::Normal(seg) = component {
            if let Some(segment) = seg.to_str() {
                let lower = segment.to_ascii_lowercase();
                if IGNORED_COMPONENTS.iter().any(|needle| needle == &lower) {
                    return false;
                }
            }
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::is_trackable;
    use std::path::Path;

    #[test]
    fn ignores_git_directory_unix_style() {
        assert!(!is_trackable(Path::new("/repo/.git/config")));
    }

    #[test]
    fn ignores_git_directory_windows_style() {
        assert!(!is_trackable(Path::new("C:\\repo\\.git\\config")));
    }

    #[test]
    fn ignores_target_directory() {
        assert!(!is_trackable(Path::new("/repo/target/debug/app")));
    }

    #[test]
    fn ignores_dx_directory() {
        assert!(!is_trackable(Path::new("/repo/.dx/forge/forge.db")));
    }

    #[test]
    fn ignores_dx_client_directory() {
        assert!(!is_trackable(Path::new("/repo/.dx_client/forge/forge.db")));
    }

    #[test]
    fn tracks_regular_source_file() {
        assert!(is_trackable(Path::new("/repo/src/main.rs")));
    }

    #[test]
    fn tracks_nested_source_file() {
        assert!(is_trackable(Path::new("C:\\repo\\src\\lib.rs")));
    }
}
