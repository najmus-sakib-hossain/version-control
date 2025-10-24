use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use ahash::AHashMap; // Using a faster hasher for our state map
use anyhow::Result;
use colored::*;
use ignore::WalkBuilder;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use similar::{ChangeTag, TextDiff};

fn main() -> Result<()> {
    println!("{}", "🔥 Starting Forge Watcher...".bold().yellow());

    // --- Phase 1: Initialization ---
    // Use gitoxide to find the root of the workdir (where .git is).
    // This is faster and more reliable than manually searching upwards.
    let repo = gix::discover(".")?;
    let work_dir = repo.workdir().ok_or_else(|| {
        anyhow::anyhow!(
            "Failed to find a Git work directory. Please run forge in a git repository."
        )
    })?;

    println!("{} {}", "Watching project root:".cyan(), work_dir.display());

    let forge_dir = work_dir.join(".dx").join("forge");
    if !forge_dir.exists() {
        fs::create_dir_all(&forge_dir)?;
    }

    // --- Phase 2: Initial State Caching ---
    // We need a baseline to compare future changes against.
    // We build this cache once at the start.
    let mut state_cache: AHashMap<PathBuf, Vec<u8>> = AHashMap::default();

    println!("{}", "🔍 Building initial file state cache...".cyan());
    let init_start = Instant::now();

    // Use `ignore` for a gitignore-aware file walk.
    let walker = WalkBuilder::new(&work_dir).build();
    for entry in walker {
        match entry {
            Ok(entry) => {
                if entry.file_type().map_or(false, |ft| ft.is_file()) {
                    let path = entry.path().to_path_buf();
                    if let Ok(contents) = fs::read(&path) {
                        state_cache.insert(path, contents);
                    }
                }
            }
            Err(e) => eprintln!("{} {}", "Error during initial scan:".red(), e),
        }
    }

    println!(
        "{} Initial scan completed in {:?}, watching {} files.",
        "✅".green(),
        init_start.elapsed(),
        state_cache.len()
    );

    // --- Phase 3: The Watch Loop ---
    // This is the heart of the application. It needs to be extremely low-latency.
    let (tx, rx) = channel();

    // `notify`'s RecommendedWatcher uses the most efficient OS-specific backend (inotify, FSEvent, etc.)
    let mut watcher = RecommendedWatcher::new(tx, notify::Config::default())?;
    watcher.watch(work_dir, RecursiveMode::Recursive)?;

    println!(
        "{}",
        "\n🚀 Watcher is active. Waiting for file system changes..."
            .bold()
            .green()
    );

    // Loop forever, processing events as they come in.
    for res in rx {
        // Start the timer *immediately* upon receiving the event from the channel.
        let detection_start = Instant::now();

        match res {
            Ok(event) => {
                // We only care about data changes, creations, and removals for this logic.
                // We handle multiple paths in case of renames.
                for path in event.paths {
                    // Ignore changes within our own `.dx/forge` directory to prevent feedback loops.
                    if path.starts_with(&forge_dir) {
                        continue;
                    }

                    match &event.kind {
                        // --- FILE MODIFIED ---
                        notify::EventKind::Modify(_) => {
                            if !path.is_file() {
                                continue;
                            }

                            // Retrieve old state from our super-fast in-memory cache.
                            let old_content = state_cache.get(&path).cloned().unwrap_or_default();

                            // Read new state from disk. This is our main I/O bottleneck.
                            if let Ok(new_content) = fs::read(&path) {
                                // Don't process if content is identical (e.g., `touch` command)
                                if new_content == old_content {
                                    continue;
                                }

                                let time_to_detect = detection_start.elapsed();
                                log_diff(&path, &old_content, &new_content, time_to_detect);

                                // Update the cache with the new state.
                                state_cache.insert(path, new_content);
                            }
                        }

                        // --- FILE CREATED ---
                        notify::EventKind::Create(_) => {
                            if !path.is_file() {
                                continue;
                            }

                            if let Ok(new_content) = fs::read(&path) {
                                let time_to_detect = detection_start.elapsed();
                                println!(
                                    "{} {} [{:?}]",
                                    "[CREATE]".bold().green(),
                                    path.strip_prefix(work_dir).unwrap_or(&path).display(),
                                    time_to_detect,
                                );
                                // Log the new content as a diff against an empty file.
                                log_diff(&path, &[], &new_content, Duration::ZERO);
                                state_cache.insert(path, new_content);
                            }
                        }

                        // --- FILE DELETED ---
                        notify::EventKind::Remove(_) => {
                            if state_cache.remove(&path).is_some() {
                                let time_to_detect = detection_start.elapsed();
                                println!(
                                    "{} {} [{:?}]",
                                    "[DELETE]".bold().red(),
                                    path.strip_prefix(work_dir).unwrap_or(&path).display(),
                                    time_to_detect,
                                );
                            }
                        }
                        _ => {} // Ignore other event types like Access, Metadata changes etc.
                    }
                }
            }
            Err(e) => eprintln!("{} {}", "Watch error:".red(), e),
        }
    }

    Ok(())
}

/// A helper function to compute and log the diff between two file contents.
fn log_diff(path: &Path, old_content: &[u8], new_content: &[u8], detection_time: Duration) {
    let old_text = String::from_utf8_lossy(old_content);
    let new_text = String::from_utf8_lossy(new_content);

    // `TextDiff` from the `similar` crate is very efficient.
    let diff = TextDiff::from_lines(&old_text, &new_text);

    let mut added_lines = 0;
    let mut removed_lines = 0;

    // We only iterate through the changes once to calculate stats and prepare for printing.
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Delete => removed_lines += 1,
            ChangeTag::Insert => added_lines += 1,
            ChangeTag::Equal => (),
        }
    }

    let path_display = path.file_name().unwrap_or_default().to_string_lossy();
    let time_str = format!("[~{:.2?}]", detection_time).dimmed();

    println!(
        "{} {} {} {} {} {}",
        "[CHANGE]".bold().blue(),
        path_display,
        format!("+{}", added_lines).green(),
        format!("-{}", removed_lines).red(),
        "lines".dimmed(),
        time_str
    );

    // Optional: Print the actual diff for small changes
    if added_lines + removed_lines < 30 {
        // Only print diff for smaller changes
        for change in diff.iter_all_changes() {
            let sign = match change.tag() {
                ChangeTag::Delete => "-".red(),
                ChangeTag::Insert => "+".green(),
                ChangeTag::Equal => " ".dimmed(),
            };
            print!("{} {}", sign, change);
        }
        println!("----------------------------------------");
    }
}
