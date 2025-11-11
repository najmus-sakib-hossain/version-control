//! Simple example showing how to use forge as a library
//!
//! This demonstrates the dual-event system:
//! - Rapid events: <35µs for instant UI feedback
//! - Quality events: <60µs with full operation details

use anyhow::Result;
use forge::ForgeWatcher;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 Forge Watcher - Simple Example");
    println!("Watching current directory for changes...\n");

    // Create watcher for current directory
    let watcher = ForgeWatcher::new(".", false, vec![]).await?;

    // Note: In production, you'd use a callback to handle events
    // For now, the watcher prints events internally

    println!("✓ Watcher started successfully!");
    println!("Try editing a file to see dual events:\n");
    println!("  ⚡ [RAPID <35µs] - Instant change notification");
    println!("  ✨ [QUALITY <60µs] - Full operation details\n");

    watcher.run().await?;

    Ok(())
}
