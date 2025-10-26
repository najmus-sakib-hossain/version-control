//! # Forge - Ultra-Fast File Watcher Library
//!
//! Production-ready file watcher with dual-mode event system optimized for DX tools.
//!
//! ## Features
//!
//! - **Rapid Events** (<35µs): Ultra-fast change notifications for immediate UI feedback
//! - **Quality Events** (<60µs): Complete operation details with line numbers and diffs
//! - **Zero Environment Variables**: Production-ready with optimal hardcoded settings
//! - **CRDT-based**: Conflict-free replicated data types for distributed sync
//! - **Memory-mapped I/O**: Leverages OS page cache for sub-microsecond reads
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use forge::{ForgeWatcher, ForgeEvent};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let watcher = ForgeWatcher::new(".", |event| {
//!         match event {
//!             ForgeEvent::Rapid { path, time_us } => {
//!                 println!("⚡ File changed: {} ({}µs)", path, time_us);
//!             }
//!             ForgeEvent::Quality { path, operations, time_us, .. } => {
//!                 println!("📊 {} operations in {} ({}µs)", operations.len(), path, time_us);
//!             }
//!         }
//!     }).await?;
//!     
//!     watcher.run().await?;
//!     Ok(())
//! }
//! ```

pub mod context;
pub mod crdt;
pub mod server;
pub mod storage;
pub mod sync;
pub mod watcher;

// Re-export main types for library consumers
pub use crdt::{Operation, OperationType, Position};
pub use watcher::{ForgeEvent, ForgeWatcher, RapidChange, QualityChange};
pub use storage::{Database, OperationLog};

/// Library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
