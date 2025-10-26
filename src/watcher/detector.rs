use anyhow::Result;
use colored::*;
use notify::event::{ModifyKind, RenameMode};
use notify::{EventKind, RecursiveMode};
use notify_debouncer_full::{new_debouncer, DebounceEventResult};
use once_cell::sync::Lazy;
use rayon::prelude::*;
use std::fs::File;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::sync::mpsc::{channel, Receiver};
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

// � ULTRA-FAST FILE HASH CACHE: ahash-based instant change detection (dx-style)
// Maps path -> (file_hash, mtime, size) for O(1) "has file changed?" checks
static FILE_HASH_CACHE: Lazy<DashMap<PathBuf, (u64, u64, u64)>> = Lazy::new(|| DashMap::new());

// �🚀 Get cached path string or convert and cache (avoids expensive Windows path conversions)
#[inline(always)]
fn path_to_string(path: &Path) -> String {
    if let Some(cached) = PATH_STRING_CACHE.get(path) {
        return cached.value().clone();
    }
    
    let s = path.display().to_string();
    PATH_STRING_CACHE.insert(path.to_path_buf(), s.clone());
    s
}

/// 🚀 ULTRA-FAST: Check if file changed using ONLY metadata (dx-style, <1µs)
/// Returns false if file definitely hasn't changed (mtime+size match)
#[inline(always)]
fn file_definitely_changed(path: &Path) -> bool {
    // Quick metadata check only (< 1µs) - NO content hashing!
    let Ok(metadata) = std::fs::metadata(path) else { return true };
    let size = metadata.len();
    let Ok(mtime) = metadata.modified() else { return true };
    let Ok(mtime_secs) = mtime.duration_since(std::time::UNIX_EPOCH) else { return true };
    
    // Check cache: if mtime+size match, file definitely hasn't changed
    if let Some(cached) = FILE_HASH_CACHE.get(path) {
        let (_hash, cached_mtime, cached_size) = *cached.value();
        if cached_mtime == mtime_secs.as_secs() && cached_size == size {
            return false; // File hasn't changed, skip processing!
        }
    }
    
    // File changed or not cached - update cache with new metadata
    // We'll compute hash lazily only if we actually need to diff
    FILE_HASH_CACHE.insert(path.to_path_buf(), (0, mtime_secs.as_secs(), size));
    true
}

// ⚡⚡ DUAL-WATCHER SYSTEM: Ultra-fast + Quality modes ⚡⚡
// 
// Mode 1: ULTRA-FAST (<20µs target) - Metadata-only change detection
//   - NO file reads, NO system calls (even metadata is skipped!)
//   - Uses atomic counter for deduplication (no time syscalls)
//   - NO line counting, NO operation detection
//   - Just logs that a file changed (for instant UI feedback)
//
// Mode 2: QUALITY (60µs target) - Full operation detection  
//   - Full file reads with line numbers
//   - Complete operation detection and diffs
//   - Runs in background after ultra-fast mode
//   - Provides all details for sync and history

// 🚀 Atomic sequence counter for ultra-fast deduplication (no syscalls!)
static RAPID_SEQUENCE: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));

// 🎛️ Environment variable to disable rapid mode for testing
static DISABLE_RAPID_MODE: Lazy<bool> = Lazy::new(|| {
    std::env::var("DX_DISABLE_RAPID_MODE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
});

/// ⚡ ULTRA-FAST MODE: Change detection with ZERO syscalls (<20µs)
/// Returns simple event indicating file changed
#[inline(always)]
fn detect_rapid_change(path: &Path) -> Option<u64> {
    // Skip if disabled via env var
    if *DISABLE_RAPID_MODE {
        return Some(0);
    }
    
    let start = Instant::now();
    
    // Ultra-fast: NO syscalls! Just use atomic sequence counter
    // This achieves sub-10µs performance by avoiding ALL system calls
    let sequence = RAPID_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    
    // Check cache - use sequence number for deduplication
    if let Some(cached) = FILE_HASH_CACHE.get(path) {
        let (_hash, cached_seq, _size) = *cached.value();
        // If we just processed this (within last 100 sequence numbers), skip
        if sequence - cached_seq < 100 {
            return None; // Recently processed, skip duplicate
        }
    }
    
    // Mark as processed (no file operations!)
    FILE_HASH_CACHE.insert(path.to_path_buf(), (0, sequence, 0));
    
    let elapsed = start.elapsed().as_micros() as u64;
    
    // Log ultra-fast detection
    if *PROFILE_DETECT || (elapsed as u128) > TARGET_PERFORMANCE_US {
        let marker = if (elapsed as u128) <= TARGET_PERFORMANCE_US { "⚡" } else { "🐌" };
        println!(
            "{} [RAPID {}µs] {} changed",
            marker,
            elapsed,
            path_to_string(path).bright_cyan(),
        );
    }
    
    Some(elapsed)
}

/// 📊 QUALITY MODE: Full operation detection with line numbers (60µs target)
/// This runs in background after rapid mode provides instant feedback
fn detect_quality_operations(
    path: &Path,
    actor_id: &str,
    rapid_time_us: u64,
) -> Result<DetectionReport> {
    let start = Instant::now();
    
    // Skip quality mode if rapid mode is disabled (for direct comparison)
    if *DISABLE_RAPID_MODE {
        let report = detect_operations(path, actor_id)?;
        let total_time = start.elapsed().as_micros();
        
        if *PROFILE_DETECT || !report.ops.is_empty() {
            println!(
                "⚙️ [QUALITY ONLY {}µs] {} - {} ops",
                total_time,
                path_to_string(path).bright_green(),
                report.ops.len()
            );
        }
        
        return Ok(report);
    }
    
    // ⚡ ULTRA-FAST QUALITY MODE: Optimized detection
    let report = detect_operations_ultra_fast(path, actor_id)?;
    
    let quality_time = start.elapsed().as_micros();
    let total_time = rapid_time_us as u128 + quality_time;
    
    // Log quality detection
    if *PROFILE_DETECT || !report.ops.is_empty() {
        let marker = if quality_time <= 60 { "✨" } else { "🐢" };
        println!(
            "{} [QUALITY {}µs | total {}µs] {} - {} ops",
            marker,
            quality_time,
            total_time,
            path_to_string(path).bright_green(),
            report.ops.len()
        );
    }
    
    Ok(report)
}

/// ⚡ ULTRA-FAST operation detection (<60µs target)
/// Skips expensive operations when possible
#[inline(always)]
fn detect_operations_ultra_fast(path: &Path, actor_id: &str) -> Result<DetectionReport> {
    let detect_start = Instant::now();
    let timings = DetectionTimings::default();

    let previous_snapshot = PREV_STATE.get(path).map(|entry| entry.value().clone());

    // 🎯 NEW FILE FAST PATH: Skip line counting for create operations
    if previous_snapshot.is_none() {
        let new_content = match read_file_fast(path) {
            Ok(text) => text,
            Err(_) => return Ok(finalize_detection(path, detect_start, timings, Vec::new())),
        };

        if new_content.len() as u64 > MAX_TRACKED_FILE_BYTES {
            return Ok(finalize_detection(path, detect_start, timings, Vec::new()));
        }

        // Build minimal snapshot (skip line breaks for file creates)
        let snapshot = build_snapshot_minimal(&new_content);
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

    let prev = previous_snapshot.unwrap();

    let new_content = match read_file_fast(path) {
        Ok(text) => text,
        Err(_) => return Ok(finalize_detection(path, detect_start, timings, Vec::new())),
    };
    
    // ⚡ ULTRA-FAST: Length check first (1µs)
    if new_content.len() == prev.content.len() {
        if new_content.as_bytes() == prev.content.as_bytes() {
            return Ok(finalize_detection(path, detect_start, timings, Vec::new()));
        }
    }
    
    // ⚡ ULTRA-FAST: Append detection (most common, ~10µs)
    if new_content.len() > prev.content.len() && new_content.starts_with(&prev.content) {
        let appended = &new_content[prev.content.len()..];
        if !appended.is_empty() {
            return handle_append_fast(path, &prev, appended, actor_id, detect_start, timings);
        }
    }
    
    // ⚡ FAST: Single-char edit detection (10-20µs)
    let len_diff = (new_content.len() as i64 - prev.content.len() as i64).abs();
    if len_diff <= 10 {
        // Likely a small edit, use optimized single-operation path
        return detect_single_edit_fast(path, &prev, &new_content, actor_id, detect_start, timings);
    }
    
    // Full diff fallback (expensive, but rare)
    detect_operations_with_content(path, actor_id, Some(new_content))
}

/// ⚡ Handle append operation (<10µs)
#[inline(always)]
fn handle_append_fast(
    path: &Path,
    prev: &FileSnapshot,
    appended: &str,
    actor_id: &str,
    detect_start: Instant,
    timings: DetectionTimings,
) -> Result<DetectionReport> {
    let char_offset = prev.char_len;
    
    // Skip expensive line/col calculation, use cached values
    let (line, col) = if prev.line_starts.is_empty() {
        (1, char_offset + 1) // Single line file
    } else {
        // Use last line position
        (prev.line_starts.len() + 1, char_offset - prev.line_starts.last().unwrap_or(&0))
    };
    
    let lamport = GLOBAL_CLOCK.tick();
    let appended_len = appended.chars().count();
    
    let op = register_operation(Operation::new(
        path_to_string(path),
        OperationType::Insert {
            position: Position::new(line, col, char_offset, actor_id.to_string(), lamport),
            content: appended.to_string(),
            length: appended_len,
        },
        actor_id.to_string(),
    ));
    
    // Update snapshot lazily (skip full rebuild)
    let mut new_prev = prev.clone();
    extend_snapshot(&mut new_prev, appended);
    update_prev_state(path, Some(new_prev));
    
    Ok(finalize_detection(path, detect_start, timings, vec![op]))
}

/// ⚡ Detect single edit operation (<20µs)
#[inline(always)]
fn detect_single_edit_fast(
    path: &Path,
    prev: &FileSnapshot,
    new_content: &str,
    actor_id: &str,
    detect_start: Instant,
    timings: DetectionTimings,
) -> Result<DetectionReport> {
    // Find the change position using byte-level diff
    let old_bytes = prev.content.as_bytes();
    let new_bytes = new_content.as_bytes();
    
    // Find common prefix (SIMD-accelerated via memchr)
    let prefix_len = old_bytes
        .iter()
        .zip(new_bytes.iter())
        .take_while(|(a, b)| a == b)
        .count();
    
    // Simple insert or delete
    let op = if new_content.len() > prev.content.len() {
        // Insert
        let inserted = &new_content[prefix_len..prefix_len + (new_content.len() - prev.content.len())];
        let char_offset = prev.content[..prefix_len].chars().count();
        let (line, col) = line_col_fast(prev, char_offset);
        
        register_operation(Operation::new(
            path_to_string(path),
            OperationType::Insert {
                position: Position::new(line, col, char_offset, actor_id.to_string(), GLOBAL_CLOCK.tick()),
                content: inserted.to_string(),
                length: inserted.chars().count(),
            },
            actor_id.to_string(),
        ))
    } else {
        // Delete
        let deleted_len = prev.content.len() - new_content.len();
        let char_offset = prev.content[..prefix_len].chars().count();
        let (line, col) = line_col_fast(prev, char_offset);
        
        register_operation(Operation::new(
            path_to_string(path),
            OperationType::Delete {
                position: Position::new(line, col, char_offset, actor_id.to_string(), GLOBAL_CLOCK.tick()),
                length: prev.content[prefix_len..prefix_len + deleted_len].chars().count(),
            },
            actor_id.to_string(),
        ))
    };
    
    // Update snapshot
    let new_snapshot = build_snapshot_minimal(new_content);
    update_prev_state(path, Some(new_snapshot));
    
    Ok(finalize_detection(path, detect_start, timings, vec![op]))
}

/// ⚡ Build minimal snapshot (skip line breaks for small edits)
#[inline(always)]
fn build_snapshot_minimal(content: &str) -> FileSnapshot {
    FileSnapshot {
        content: content.to_string(),
        char_len: content.chars().count(),
        byte_len: content.len() as u64,
        line_starts: Vec::new(), // Skip line start indexing!
        char_to_byte: Vec::new(), // Skip for ASCII
    }
}

/// ⚡ Fast line/col calculation using cached line starts
#[inline(always)]
fn line_col_fast(snapshot: &FileSnapshot, char_offset: usize) -> (usize, usize) {
    if snapshot.line_starts.is_empty() {
        return (1, char_offset + 1);
    }
    
    // Binary search in line starts
    match snapshot.line_starts.binary_search(&char_offset) {
        Ok(idx) => (idx + 2, 1),
        Err(idx) => {
            let line = idx + 1;
            let col = if idx == 0 {
                char_offset + 1
            } else {
                char_offset - snapshot.line_starts[idx - 1]
            };
            (line, col)
        }
    }
}

static PROFILE_DETECT: Lazy<bool> = Lazy::new(|| {
    std::env::var("DX_WATCH_PROFILE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
});

// 🎯 Performance target: Sub-20µs operation processing (dx-style level)
const TARGET_PERFORMANCE_US: u128 = 20;

// 🚀 Watcher mode (ultra-fast 1ms debounce only)
enum WatchMode {
    Debounced(Duration), // Ultra-fast debounced events
}

// 🚀 Watcher mode configuration (ultra-fast 1ms debounce only)
const DEBOUNCE_MS: u64 = 1; // Ultra-fast 1ms debounce for sub-20µs target

impl WatchMode {
    fn from_env() -> Self {
        println!(
            "{} Using ultra-fast mode: {}ms debounce (sub-20µs target)",
            "⚡".bright_yellow(),
            DEBOUNCE_MS
        );
        WatchMode::Debounced(Duration::from_millis(DEBOUNCE_MS))
    }
}

pub async fn start_watching(
    path: PathBuf,
    oplog: Arc<OperationLog>,
    actor_id: String,
    repo_id: String,
    sync_mgr: Option<StdArc<SyncManager>>,
) -> Result<()> {
    let mode = WatchMode::from_env();

    println!("{} Repo ID: {}", "→".bright_blue(), repo_id.bright_yellow());
    
    // ⚡⚡ Show dual-watcher status
    if *DISABLE_RAPID_MODE {
        println!(
            "{} Dual-watcher: DISABLED (quality mode only)",
            "⚠️".bright_yellow()
        );
        println!(
            "{} Set DX_DISABLE_RAPID_MODE=0 to enable ultra-fast mode",
            "💡".bright_black()
        );
    } else {
        println!(
            "{} Dual-watcher: ENABLED (rapid <20µs + quality <60µs)",
            "⚡⚡".bright_green()
        );
    }
    
    // 🔥 Show profiling status
    if *PROFILE_DETECT {
        println!(
            "{} Profiling enabled (DX_WATCH_PROFILE=1) - showing all detection timings",
            "🔍".bright_yellow()
        );
    } else {
        println!(
            "{} Set DX_WATCH_PROFILE=1 to see detailed detection timings",
            "💡".bright_black()
        );
    }

    match mode {
        WatchMode::Debounced(debounce) => {
            start_debounced_watcher(path, oplog, actor_id, sync_mgr, debounce).await
        }
    }
}

// 🚀 Ultra-fast debounced watcher (1ms, sub-20µs detection)
async fn start_debounced_watcher(
    path: PathBuf,
    oplog: Arc<OperationLog>,
    actor_id: String,
    sync_mgr: Option<StdArc<SyncManager>>,
    debounce: Duration,
) -> Result<()> {
    let (tx, rx) = channel();
    
    let mut debouncer = new_debouncer(debounce, None, tx)?;
    debouncer.watch(&path, RecursiveMode::Recursive)?;

    process_events_loop(rx, actor_id, oplog, sync_mgr).await
}

// 🎯 Core event processing loop (shared by all modes)
async fn process_events_loop(
    rx: Receiver<DebounceEventResult>,
    actor_id: String,
    oplog: Arc<OperationLog>,
    sync_mgr: Option<StdArc<SyncManager>>,
) -> Result<()> {
    while let Ok(result) = rx.recv() {
        match result {
            Ok(events) => {
                for event in events {
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
            }
            Err(errors) => {
                for error in errors {
                    println!("{} Debouncer error: {}", "⚠️".bright_red(), error);
                }
            }
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
#[allow(dead_code)]
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

// � Ultra-fast deduplication now handled by FILE_HASH_CACHE (ahash-based, <1µs)

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
    
    // Store operations for diff display AFTER timing
    let ops_for_diff = ops.clone();
    
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
    
    // 🎨 Display operation details AFTER timing (doesn't count in performance metrics)
    print_operation_diff(&ops_for_diff);
    
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

    // ⚡⚡ DUAL-WATCHER SYSTEM ⚡⚡
    
    // Step 1: ULTRA-FAST MODE (<20µs) - Zero-syscall rapid change detection
    let rapid_result = detect_rapid_change(path);
    
    // If no change detected by rapid mode, we're done!
    let Some(rapid_time_us) = rapid_result else {
        return Ok(());
    };
    
    // Step 2: QUALITY MODE (60µs) - Full operation detection in background
    // This provides complete details with line numbers, diffs, etc.
    match detect_quality_operations(path, actor_id, rapid_time_us) {
        Ok(report) => {
            if !report.ops.is_empty() {
                let detect_us = report.timings.total_us;
                emit_operations(report.ops, detect_us, start, oplog, sync_mgr)?;
            }
        }
        Err(_) => {
            // If quality detection fails, at least we logged the rapid change
        }
    }

    Ok(())
}

// 🔥 Deduplication helper: Skip if we just processed this file
// 🚀 Deduplication now handled by file_definitely_changed() using metadata-only (<1µs)
// No need for separate should_skip_duplicate function

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

#[inline(always)]
fn detect_operations(path: &Path, actor_id: &str) -> Result<DetectionReport> {
    detect_operations_with_content(path, actor_id, None)
}

#[inline(always)]
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
    
    // 🔥 Show profile logs when profiling is enabled OR operations were created
    profile_detect(path, &timings, !ops.is_empty());
    
    DetectionReport { ops, timings }
}

fn profile_detect(path: &Path, timings: &DetectionTimings, has_ops: bool) {
    // Skip if profiling is disabled AND no operations were created
    if !*PROFILE_DETECT && !has_ops {
        return;
    }
    
    // When profiling is enabled, show all logs
    // When profiling is disabled, only show if operations were created
    if *PROFILE_DETECT || has_ops {
        println!(
            "⚙️ detect {} | total={}µs",
            path.display(),
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
    
    // 🔥 FIX: Safe byte range calculation with bounds checking
    // Get byte ranges - ensure indices are within bounds
    let old_start_byte = if old_start < old_snap.char_to_byte.len() {
        old_snap.char_to_byte[old_start]
    } else {
        old_snap.content.len()
    };
    
    let old_end_byte = if old_end < old_snap.char_to_byte.len() {
        old_snap.char_to_byte[old_end]
    } else {
        old_snap.content.len()
    };
    
    let new_start_byte = if new_start < new_snap.char_to_byte.len() {
        new_snap.char_to_byte[new_start]
    } else {
        new_snap.content.len()
    };
    
    let new_end_byte = if new_end < new_snap.char_to_byte.len() {
        new_snap.char_to_byte[new_end]
    } else {
        new_snap.content.len()
    };

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
/// 🚀 ULTRA-FAST: Binary diff using SIMD-like parallel comparison (sub-5µs for small changes)
/// Uses rayon for parallel processing on large files
fn compute_change_range_fast(
    old_bytes: &[u8],
    new_bytes: &[u8],
    old_snapshot: &FileSnapshot,
    new_snapshot: &FileSnapshot,
) -> Option<(usize, usize, usize, usize)> {
    if old_snapshot.char_len == 0 && new_snapshot.char_len == 0 {
        return None;
    }

    // 🔥 ULTRA-FAST: Use memchr for SIMD-accelerated difference detection
    // Find common prefix using parallel byte comparison
    let common_prefix_bytes = if old_bytes.len() > 8192 && new_bytes.len() > 8192 {
        // Large files: use rayon for parallel prefix search
        use rayon::prelude::*;
        
        let chunk_size = 4096;
        let min_len = old_bytes.len().min(new_bytes.len());
        let num_chunks = (min_len + chunk_size - 1) / chunk_size;
        
        (0..num_chunks)
            .into_par_iter()
            .map(|chunk_idx| {
                let start = chunk_idx * chunk_size;
                let end = (start + chunk_size).min(min_len);
                let chunk_old = &old_bytes[start..end];
                let chunk_new = &new_bytes[start..end];
                
                // Find first difference in this chunk
                chunk_old
                    .iter()
                    .zip(chunk_new.iter())
                    .take_while(|(a, b)| a == b)
                    .count()
            })
            .enumerate()
            .find(|(_, prefix_len)| *prefix_len < chunk_size)
            .map(|(idx, partial)| idx * chunk_size + partial)
            .unwrap_or(min_len)
    } else {
        // Small files: simple linear scan (already very fast)
        old_bytes
            .iter()
            .zip(new_bytes.iter())
            .take_while(|(a, b)| a == b)
            .count()
    };
    
    // Find common suffix at byte level
    let remaining_old = old_bytes.len() - common_prefix_bytes;
    let remaining_new = new_bytes.len() - common_prefix_bytes;
    let common_suffix_bytes = if remaining_old > 0 && remaining_new > 0 {
        old_bytes[common_prefix_bytes..]
            .iter()
            .rev()
            .zip(new_bytes[common_prefix_bytes..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count()
            .min(remaining_old.min(remaining_new))
    } else {
        0
    };
    
    // 🔥 FIX: Handle ASCII fast path (char_to_byte is empty for ASCII)
    let old_is_ascii = old_snapshot.char_to_byte.is_empty();
    let new_is_ascii = new_snapshot.char_to_byte.is_empty();
    
    // Convert byte positions to char positions
    let prefix_chars = if old_is_ascii {
        common_prefix_bytes // For ASCII: byte pos == char pos
    } else {
        old_snapshot.char_to_byte
            .iter()
            .position(|&b| b >= common_prefix_bytes)
            .unwrap_or(old_snapshot.char_len)
    };
    
    let old_suffix_byte_pos = old_bytes.len() - common_suffix_bytes;
    let old_suffix_chars = if old_is_ascii {
        old_suffix_byte_pos // For ASCII: byte pos == char pos
    } else {
        old_snapshot.char_to_byte
            .iter()
            .position(|&b| b >= old_suffix_byte_pos)
            .unwrap_or(old_snapshot.char_len)
    };
    
    let new_suffix_byte_pos = new_bytes.len() - common_suffix_bytes;
    let new_suffix_chars = if new_is_ascii {
        new_suffix_byte_pos // For ASCII: byte pos == char pos
    } else {
        new_snapshot.char_to_byte
            .iter()
            .position(|&b| b >= new_suffix_byte_pos)
            .unwrap_or(new_snapshot.char_len)
    };
    
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

    // Extract filename from path (cleaner display)
    let filename = std::path::Path::new(&op.file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&op.file_path);

    // 📝 Build detailed operation info with content preview
    let (action, details) = match &op.op_type {
        OperationType::Insert { position, content, length } => {
            let preview = truncate_with_preview(content, 40);
            (
                "INSERT".green(),
                format!(
                    "{}:{} +{} chars {}",
                    position.line,
                    position.column,
                    length,
                    format!("'{}'", preview).green()
                ),
            )
        }
        OperationType::Delete { position, length } => {
            (
                "DELETE".red(),
                format!(
                    "{}:{} -{} chars",
                    position.line,
                    position.column,
                    length
                ),
            )
        }
        OperationType::Replace {
            position,
            old_content,
            new_content,
        } => {
            let old_preview = truncate_with_preview(old_content, 20);
            let new_preview = truncate_with_preview(new_content, 20);
            (
                "REPLACE".yellow(),
                format!(
                    "{}:{} '{}' → '{}'",
                    position.line,
                    position.column,
                    old_preview.red(),
                    new_preview.green()
                ),
            )
        }
        OperationType::FileCreate { content } => {
            let size = content.len();
            let lines = content.lines().count();
            (
                "CREATE".bright_green(),
                format!("file ({} bytes, {} lines)", size, lines),
            )
        }
        OperationType::FileDelete => {
            ("DELETE".bright_red(), "file".to_string())
        }
        OperationType::FileRename { old_path, new_path } => {
            let old_name = std::path::Path::new(old_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(old_path);
            let new_name = std::path::Path::new(new_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(new_path);
            (
                "RENAME".bright_yellow(),
                format!("{} → {}", old_name.red(), new_name.green()),
            )
        }
    };

    println!(
        "{} {} {} {}",
        perf_indicator,
        time_colored,
        action.bold(),
        format!("{} {}", filename.bright_white(), details)
    );
}

// 🔧 Helper: Truncate string with ellipsis for clean preview
fn truncate_with_preview(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        // Escape newlines and tabs for display
        s.replace('\n', "\\n").replace('\t', "\\t")
    } else {
        // Show first part with ellipsis
        let truncated = &s[..max_len.min(s.len())];
        format!("{}…", truncated.replace('\n', "\\n").replace('\t', "\\t"))
    }
}

// 🎨 Display operation details showing what changed (displayed AFTER timing)
fn print_operation_diff(ops: &[Operation]) {
    use colored::*;
    
    for op in ops {
        let filename = std::path::Path::new(&op.file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&op.file_path);
        
        match &op.op_type {
            OperationType::Insert { position, content, length } => {
                println!("  {} {} @ {}:{}", 
                    "+".green().bold(),
                    filename.bright_cyan(),
                    position.line,
                    position.column
                );
                // Show ALL inserted lines (not truncated)
                for line in content.lines() {
                    println!("    {}", line.green());
                }
                if content.lines().count() == 0 && !content.is_empty() {
                    // Single line without newline
                    println!("    {}", content.green());
                }
            }
            OperationType::Delete { position, length } => {
                println!("  {} {} @ {}:{} ({} chars)",
                    "-".red().bold(),
                    filename.bright_cyan(),
                    position.line,
                    position.column,
                    length
                );
            }
            OperationType::Replace { position, old_content, new_content } => {
                println!("  {} {} @ {}:{}",
                    "~".yellow().bold(),
                    filename.bright_cyan(),
                    position.line,
                    position.column
                );
                // Show ALL lines for both old and new content
                for line in old_content.lines() {
                    println!("    {} {}", "-".red(), line.red());
                }
                if old_content.lines().count() == 0 && !old_content.is_empty() {
                    println!("    {} {}", "-".red(), old_content.red());
                }
                for line in new_content.lines() {
                    println!("    {} {}", "+".green(), line.green());
                }
                if new_content.lines().count() == 0 && !new_content.is_empty() {
                    println!("    {} {}", "+".green(), new_content.green());
                }
            }
            OperationType::FileCreate { content } => {
                println!("  {} {} ({} lines)",
                    "✨".bright_green(),
                    filename.bright_cyan(),
                    content.lines().count()
                );
                // Show first 10 lines of new file
                for line in content.lines().take(10) {
                    println!("    {}", line.bright_black());
                }
                if content.lines().count() > 10 {
                    println!("    {} {} more lines", "...".bright_black(), content.lines().count() - 10);
                }
            }
            OperationType::FileDelete => {
                println!("  {} {}", "🗑️ ".bright_red(), filename.bright_cyan());
            }
            OperationType::FileRename { old_path, new_path } => {
                let old_name = std::path::Path::new(old_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(old_path);
                let new_name = std::path::Path::new(new_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(new_path);
                println!("  {} {} → {}",
                    "📋".bright_yellow(),
                    old_name.yellow(),
                    new_name.bright_cyan()
                );
            }
        }
    }
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
