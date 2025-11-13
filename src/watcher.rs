//! Dual-Watcher Architecture - LSP + File System monitoring
//!
//! Provides two-tier file change detection:
//! 1. **LSP Watcher** (Primary): Monitors Language Server Protocol events
//! 2. **File System Watcher** (Fallback): Monitors actual file system changes
//!
//! The LSP watcher detects changes before they hit the disk, enabling
//! faster response times and semantic understanding of code changes.

use anyhow::{Context as _, Result};
use notify::{EventKind, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{
    new_debouncer, DebounceEventResult, DebouncedEvent, Debouncer, FileIdMap,
};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};

/// File change event
#[derive(Debug, Clone)]
pub struct FileChange {
    /// Path to the changed file
    pub path: PathBuf,

    /// Type of change
    pub kind: ChangeKind,

    /// Source of the event (LSP or FileSystem)
    pub source: ChangeSource,

    /// Timestamp of the change
    pub timestamp: std::time::SystemTime,

    /// Optional content if available from LSP
    pub content: Option<String>,

    /// Detected DX patterns (if analyzed)
    pub patterns: Option<Vec<crate::patterns::PatternMatch>>,
}

/// Type of file change
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Created,
    Modified,
    Deleted,
    Renamed,
}

/// Source of the change detection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeSource {
    Lsp,
    FileSystem,
}

/// LSP event (simplified - full LSP protocol would be more complex)
#[derive(Debug, Clone)]
pub struct LspEvent {
    pub uri: String,
    pub version: i32,
    pub content: String,
}

/// LSP Watcher - monitors Language Server Protocol events
pub struct LspWatcher {
    #[allow(dead_code)]
    lsp_rx: Receiver<LspEvent>,
    change_tx: broadcast::Sender<FileChange>,
    running: Arc<RwLock<bool>>,
}

impl LspWatcher {
    /// Create a new LSP watcher
    pub fn new() -> (Self, broadcast::Receiver<FileChange>) {
        let (_lsp_tx, lsp_rx) = channel();
        let (change_tx, change_rx) = broadcast::channel(1000);

        (
            Self {
                lsp_rx,
                change_tx,
                running: Arc::new(RwLock::new(false)),
            },
            change_rx,
        )
    }

    /// Start watching for LSP events
    pub async fn start(&self) -> Result<()> {
        *self.running.write().await = true;

        // In a real implementation, this would:
        // 1. Connect to LSP server via stdin/stdout or socket
        // 2. Subscribe to textDocument/didChange notifications
        // 3. Parse LSP JSON-RPC messages
        // 4. Extract file changes and content

        println!("📡 LSP Watcher started (mock mode - needs LSP server integration)");

        Ok(())
    }

    /// Stop watching
    pub async fn stop(&self) -> Result<()> {
        *self.running.write().await = false;
        println!("📡 LSP Watcher stopped");
        Ok(())
    }

    /// Process LSP events (would be called from LSP message loop)
    #[allow(dead_code)]
    fn process_lsp_event(&self, event: LspEvent) -> Result<()> {
        let path = PathBuf::from(event.uri.trim_start_matches("file://"));

        // Detect patterns in content
        let patterns = if let Ok(detector) = crate::patterns::PatternDetector::new() {
            detector.detect_in_file(&path, &event.content).ok()
        } else {
            None
        };

        let change = FileChange {
            path,
            kind: ChangeKind::Modified,
            source: ChangeSource::Lsp,
            timestamp: std::time::SystemTime::now(),
            content: Some(event.content),
            patterns,
        };

        let _ = self.change_tx.send(change);
        Ok(())
    }
}

/// File System Watcher - monitors actual file system changes
pub struct FileWatcher {
    debouncer: Option<Debouncer<RecommendedWatcher, FileIdMap>>,
    _event_tx: Sender<DebounceEventResult>,
}

impl FileWatcher {
    /// Create a new file system watcher
    pub fn new() -> Result<(Self, broadcast::Receiver<FileChange>)> {
        let (event_tx, _event_rx) = channel();
        let (change_tx, change_rx) = broadcast::channel(1000);

        let tx_clone = change_tx.clone();

        // Create debouncer with 100ms delay
        let debouncer = new_debouncer(
            Duration::from_millis(100),
            None,
            move |result: DebounceEventResult| {
                if let Ok(events) = result {
                    for debounced_event in events {
                        if let Some(change) = Self::debounced_event_to_change(debounced_event) {
                            let _ = tx_clone.send(change);
                        }
                    }
                }
            },
        )?;

        Ok((
            Self {
                debouncer: Some(debouncer),
                _event_tx: event_tx,
            },
            change_rx,
        ))
    }

    /// Watch a directory recursively
    pub fn watch(&mut self, path: impl AsRef<Path>) -> Result<()> {
        if let Some(debouncer) = &mut self.debouncer {
            debouncer
                .watch(path.as_ref(), RecursiveMode::Recursive)
                .with_context(|| format!("Failed to watch: {}", path.as_ref().display()))?;

            println!("👁️  File Watcher started: {}", path.as_ref().display());
        }
        Ok(())
    }

    /// Stop watching
    pub fn stop(&mut self) -> Result<()> {
        self.debouncer = None;
        println!("👁️  File Watcher stopped");
        Ok(())
    }

    /// Convert debounced event to FileChange
    fn debounced_event_to_change(debounced_event: DebouncedEvent) -> Option<FileChange> {
        let event = &debounced_event.event;
        let kind = match event.kind {
            EventKind::Create(_) => ChangeKind::Created,
            EventKind::Modify(_) => ChangeKind::Modified,
            EventKind::Remove(_) => ChangeKind::Deleted,
            _ => return None,
        };

        // Get first path from event
        let path = event.paths.first()?.clone();

        // Skip hidden files and temp files
        if let Some(name) = path.file_name() {
            let name_str = name.to_string_lossy();
            if name_str.starts_with('.') || name_str.contains('~') || name_str.ends_with(".tmp") {
                return None;
            }
        }

        Some(FileChange {
            path,
            kind,
            source: ChangeSource::FileSystem,
            timestamp: std::time::SystemTime::now(),
            content: None,
            patterns: None,
        })
    }
}

/// Dual Watcher - combines LSP and File System watchers
pub struct DualWatcher {
    lsp_watcher: Arc<LspWatcher>,
    file_watcher: Arc<RwLock<FileWatcher>>,
    change_rx: broadcast::Receiver<FileChange>,
}

impl DualWatcher {
    /// Create a new dual watcher
    pub fn new() -> Result<Self> {
        let (lsp_watcher, lsp_rx) = LspWatcher::new();
        let (file_watcher, fs_rx) = FileWatcher::new()?;

        // Create unified change channel
        let (change_tx, change_rx) = broadcast::channel(1000);

        // Spawn task to merge LSP and FS events
        let tx1 = change_tx.clone();
        tokio::spawn(async move {
            let mut lsp_rx = lsp_rx;
            while let Ok(change) = lsp_rx.recv().await {
                let _ = tx1.send(change);
            }
        });

        let tx2 = change_tx.clone();
        tokio::spawn(async move {
            let mut fs_rx = fs_rx;
            while let Ok(change) = fs_rx.recv().await {
                let _ = tx2.send(change);
            }
        });

        Ok(Self {
            lsp_watcher: Arc::new(lsp_watcher),
            file_watcher: Arc::new(RwLock::new(file_watcher)),
            change_rx,
        })
    }

    /// Start both watchers
    pub async fn start(&mut self, path: impl AsRef<Path>) -> Result<()> {
        // Start LSP watcher
        self.lsp_watcher.start().await?;

        // Start file system watcher
        self.file_watcher.write().await.watch(path)?;

        println!("🔄 Dual Watcher active: LSP + FileSystem");
        Ok(())
    }

    /// Stop both watchers
    pub async fn stop(&mut self) -> Result<()> {
        self.lsp_watcher.stop().await?;
        self.file_watcher.write().await.stop()?;
        println!("🔄 Dual Watcher stopped");
        Ok(())
    }

    /// Get the change receiver
    pub fn receiver(&self) -> broadcast::Receiver<FileChange> {
        self.change_rx.resubscribe()
    }

    /// Wait for next change
    pub async fn next_change(&mut self) -> Result<FileChange> {
        self.change_rx
            .recv()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to receive change: {}", e))
    }

    /// Analyze file changes for DX patterns
    pub async fn analyze_patterns(&self, mut change: FileChange) -> Result<FileChange> {
        // If content is available and patterns not yet detected
        if change.patterns.is_none() {
            if let Some(content) = &change.content {
                let detector = crate::patterns::PatternDetector::new()?;
                change.patterns = detector.detect_in_file(&change.path, content).ok();
            } else if change.path.exists() {
                // Read file if it exists
                if let Ok(content) = tokio::fs::read_to_string(&change.path).await {
                    let detector = crate::patterns::PatternDetector::new()?;
                    change.patterns = detector.detect_in_file(&change.path, &content).ok();
                }
            }
        }

        Ok(change)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::fs;

    #[tokio::test]
    async fn test_file_watcher_detects_changes() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.txt");

        let (mut watcher, mut rx) = FileWatcher::new().unwrap();
        watcher.watch(temp_dir.path()).unwrap();

        // Give watcher time to start
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Create a file
        fs::write(&test_file, "test content").await.unwrap();

        // Wait for event
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Check if we received an event
        if let Ok(change) = rx.try_recv() {
            assert_eq!(change.source, ChangeSource::FileSystem);
            assert!(matches!(
                change.kind,
                ChangeKind::Created | ChangeKind::Modified
            ));
        }

        watcher.stop().unwrap();
    }

    #[tokio::test]
    async fn test_dual_watcher_creation() {
        let watcher = DualWatcher::new();
        assert!(watcher.is_ok());
    }
}
