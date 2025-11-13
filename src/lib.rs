//! # DX Forge - Production-Ready VCS and Orchestration Engine
//!
//! Forge is the orchestration backbone for the DX tools ecosystem, providing:
//! - Content-addressable storage with SHA-256 blob hashing
//! - Git-compatible versioning with traffic branch safety system
//! - Dual-watcher architecture (LSP + File System monitoring)
//! - Tool orchestration with priority-based execution and dependency resolution
//! - Component injection for zero-bloat dependency management
//! - Semantic versioning with dependency resolution
//! - Pattern detection for dx-tools (dxButton, dxiIcon, dxfRoboto, etc.)
//! - R2 component caching and injection
//! - Production error handling with retry logic
//!
//! ## Architecture Overview
//!
//! Forge eliminates node_modules bloat by detecting code patterns via LSP,
//! injecting only needed components directly into user files, and coordinating
//! DX tool execution with traffic branch safety logic.
//!
//! ### Core Components
//!
//! - **Orchestrator**: Coordinates tool execution with lifecycle hooks, circular dependency detection
//! - **Dual-Watcher**: Monitors LSP + file system changes with pattern detection
//! - **Traffic Branch System**: Green (auto), Yellow (merge), Red (manual) for safe updates
//! - **Storage Layer**: Content-addressable blobs with R2 cloud sync
//! - **Version Manager**: Semantic versioning with compatibility checking
//! - **Pattern Detector**: Identifies dx-tool patterns in source code
//! - **Injection Manager**: Fetches and caches components from R2 storage
//!
//! ## Quick Start - Tool Development
//!
//! ```rust,no_run
//! use dx_forge::{DxTool, ExecutionContext, ToolOutput, Orchestrator};
//! use async_trait::async_trait;
//! use anyhow::Result;
//!
//! struct MyDxTool;
//!
//! #[async_trait]
//! impl DxTool for MyDxTool {
//!     fn name(&self) -> &str { "dx-mytool" }
//!     fn version(&self) -> &str { "1.0.0" }
//!     fn priority(&self) -> i32 { 50 }
//!     
//!     async fn execute(&self, ctx: &ExecutionContext) -> Result<ToolOutput> {
//!         // Your tool logic here
//!         Ok(ToolOutput::success("Done!"))
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let mut orchestrator = Orchestrator::new(".")?;
//!     orchestrator.register_tool(Box::new(MyDxTool));
//!     orchestrator.execute_all().await?;
//!     Ok(())
//! }
//! ```
//!
//! ## Quick Start - Change Detection
//!
//! ```rust,no_run
//! use dx_forge::{DualWatcher, FileChange};
//! use tokio::sync::broadcast;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let watcher = DualWatcher::new(".")?;
//!     let mut rx = watcher.subscribe();
//!     
//!     tokio::spawn(async move {
//!         watcher.start().await
//!     });
//!     
//!     while let Ok(change) = rx.recv().await {
//!         println!("Change detected: {:?} ({})", change.path, change.source);
//!     }
//!     Ok(())
//! }
//! ```

// Core modules
pub mod context;
pub mod crdt;
pub mod server;
pub mod storage;
pub mod sync;

// Legacy watcher module (for CLI compatibility)
#[path = "watcher_legacy/mod.rs"]
pub mod watcher_legacy;

// Production orchestration modules (v1.0.0)
pub mod orchestrator;
pub mod watcher;

// DX Tools support modules
pub mod version;
pub mod patterns;
pub mod injection;
pub mod error;

// Re-export orchestration types (public API)
pub use orchestrator::{
    Conflict, DxTool, ExecutionContext, Orchestrator, OrchestratorConfig, ToolOutput,
    TrafficAnalyzer, TrafficBranch,
};

pub use watcher::{ChangeKind, ChangeSource, DualWatcher, FileChange, FileWatcher, LspWatcher};

// Re-export storage types
pub use context::{ComponentStateManager, UpdateResult};
pub use crdt::{Operation, OperationType, Position};
pub use storage::{Database, OperationLog};

// Re-export DX tools support types
pub use version::{ToolInfo, ToolRegistry, ToolSource, Version, VersionReq};
pub use patterns::{DxToolType, PatternDetector, PatternMatch};
pub use injection::{CacheStats, ComponentMetadata, InjectionManager};
pub use error::{categorize_error, EnhancedError, EnhancedResult, ErrorCategory, RetryPolicy, ToEnhanced, with_retry};

// Legacy exports (deprecated in favor of new watcher module)
#[deprecated(since = "1.0.0", note = "use `watcher::DualWatcher` instead")]
pub use watcher::DualWatcher as ForgeWatcher;

/// Library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
