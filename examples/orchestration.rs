//! Example: DX Tools Orchestration Engine
//!
//! This example demonstrates how to build and coordinate multiple DX tools
//! using Forge's orchestration engine with priority-based execution,
//! dependency resolution, and traffic branch safety logic.
//!
//! Run with: cargo run --example orchestration

use anyhow::{Context, Result};
use async_trait::async_trait;
use dx_forge::{
    DualWatcher, DxTool, ExecutionContext, FileChange, Orchestrator, ToolOutput, TrafficAnalyzer,
    TrafficBranch,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::time::{sleep, Duration};

/// Example DX-Style tool: manages CSS/styling
struct DxStyleTool;

#[async_trait]
impl DxTool for DxStyleTool {
    fn name(&self) -> &str {
        "dx-style"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn priority(&self) -> i32 {
        100 // High priority - styles needed first
    }

    fn dependencies(&self) -> Vec<String> {
        vec![] // No dependencies
    }

    fn should_run(&self, ctx: &ExecutionContext) -> bool {
        // Run if any CSS or style-related files changed
        ctx.changed_files.iter().any(|f| {
            f.to_str()
                .map(|s| s.ends_with(".css") || s.contains("style"))
                .unwrap_or(false)
        })
    }

    async fn execute(&self, ctx: &ExecutionContext) -> Result<ToolOutput> {
        println!("🎨 [dx-style] Processing styles...");
        sleep(Duration::from_millis(50)).await; // Simulate work

        let mut messages = Vec::new();
        for file in &ctx.changed_files {
            messages.push(format!("Processed: {}", file.display()));
        }

        Ok(ToolOutput {
            success: true,
            message: "Styles processed successfully".to_string(),
            artifacts: HashMap::from([("css_bundle".to_string(), "styles.min.css".to_string())]),
            warnings: vec![],
            errors: vec![],
            execution_time: Duration::from_millis(50),
        })
    }
}

/// Example DX-UI tool: manages UI components
struct DxUiTool;

#[async_trait]
impl DxTool for DxUiTool {
    fn name(&self) -> &str {
        "dx-ui"
    }

    fn version(&self) -> &str {
        "2.1.0"
    }

    fn priority(&self) -> i32 {
        80 // Medium-high priority
    }

    fn dependencies(&self) -> Vec<String> {
        vec!["dx-style".to_string()] // Needs styles first
    }

    fn should_run(&self, ctx: &ExecutionContext) -> bool {
        // Run if any component files changed or if styles changed
        ctx.changed_files.iter().any(|f| {
            f.to_str()
                .map(|s| s.contains("component") || s.ends_with(".tsx") || s.ends_with(".jsx"))
                .unwrap_or(false)
        })
    }

    async fn execute(&self, ctx: &ExecutionContext) -> Result<ToolOutput> {
        println!("🧩 [dx-ui] Injecting UI components...");
        sleep(Duration::from_millis(100)).await; // Simulate work

        let mut artifacts = HashMap::new();
        artifacts.insert(
            "components_injected".to_string(),
            "dxButton, dxInput".to_string(),
        );

        Ok(ToolOutput {
            success: true,
            message: "UI components injected successfully".to_string(),
            artifacts,
            warnings: vec!["Some components not optimized".to_string()],
            errors: vec![],
            execution_time: Duration::from_millis(100),
        })
    }
}

/// Example DX-Icons tool: manages icon assets
struct DxIconsTool;

#[async_trait]
impl DxTool for DxIconsTool {
    fn name(&self) -> &str {
        "dx-icons"
    }

    fn version(&self) -> &str {
        "1.5.0"
    }

    fn priority(&self) -> i32 {
        60 // Medium priority
    }

    fn dependencies(&self) -> Vec<String> {
        vec!["dx-ui".to_string()] // Needs UI components first
    }

    fn should_run(&self, ctx: &ExecutionContext) -> bool {
        // Run if icon references detected in changes
        ctx.changed_files.iter().any(|f| {
            f.to_str()
                .map(|s| s.contains("icon") || s.contains("dxi"))
                .unwrap_or(false)
        })
    }

    async fn execute(&self, ctx: &ExecutionContext) -> Result<ToolOutput> {
        println!("🎭 [dx-icons] Processing icon references...");
        sleep(Duration::from_millis(30)).await; // Simulate work

        Ok(ToolOutput {
            success: true,
            message: "Icons injected: dxiArrowRight, dxiCheck".to_string(),
            artifacts: HashMap::from([("icons_count".to_string(), "2".to_string())]),
            warnings: vec![],
            errors: vec![],
            execution_time: Duration::from_millis(30),
        })
    }
}

/// Example DX-Check tool: runs validation and linting
struct DxCheckTool;

#[async_trait]
impl DxTool for DxCheckTool {
    fn name(&self) -> &str {
        "dx-check"
    }

    fn version(&self) -> &str {
        "3.0.0"
    }

    fn priority(&self) -> i32 {
        10 // Low priority - runs last
    }

    fn dependencies(&self) -> Vec<String> {
        vec![
            "dx-style".to_string(),
            "dx-ui".to_string(),
            "dx-icons".to_string(),
        ]
    }

    fn should_run(&self, _ctx: &ExecutionContext) -> bool {
        true // Always run validation
    }

    async fn execute(&self, ctx: &ExecutionContext) -> Result<ToolOutput> {
        println!("✅ [dx-check] Running validation...");
        sleep(Duration::from_millis(80)).await; // Simulate work

        let file_count = ctx.changed_files.len();
        Ok(ToolOutput {
            success: true,
            message: format!("Validated {} files - all checks passed", file_count),
            artifacts: HashMap::from([
                ("checks_passed".to_string(), "23".to_string()),
                ("checks_failed".to_string(), "0".to_string()),
            ]),
            warnings: vec!["Consider using dxButton instead of custom button".to_string()],
            errors: vec![],
            execution_time: Duration::from_millis(80),
        })
    }
}

/// Simple traffic analyzer for demo purposes
struct SimpleTrafficAnalyzer;

impl TrafficAnalyzer for SimpleTrafficAnalyzer {
    fn analyze_change(
        &self,
        _file: &Path,
        _old_content: &str,
        _new_content: &str,
    ) -> TrafficBranch {
        // In a real implementation, this would do:
        // 1. Parse both versions
        // 2. Detect conflicts
        // 3. Decide on traffic branch

        // For demo: randomly assign traffic status
        use std::collections::hash_map::RandomState;
        use std::hash::{BuildHasher, Hasher};

        let mut hasher = RandomState::new().build_hasher();
        hasher.write(std::process::id().to_string().as_bytes());
        let val = hasher.finish() % 3;

        match val {
            0 => TrafficBranch::Green,
            1 => TrafficBranch::Yellow(vec!["Potential style conflict".to_string()]),
            _ => TrafficBranch::Red(vec!["Breaking API change detected".to_string()]),
        }
    }
}

/// Monitor file changes with the dual-watcher
async fn monitor_changes(repo_path: &str) -> Result<()> {
    println!("\n📡 Starting Dual-Watcher (LSP + File System)...");

    let watcher = DualWatcher::new(repo_path)?;
    let mut rx = watcher.subscribe();

    // Start watcher in background
    let watch_handle = tokio::spawn(async move {
        if let Err(e) = watcher.start().await {
            eprintln!("Watcher error: {}", e);
        }
    });

    println!("   Watching for changes (Ctrl+C to stop)...\n");

    // Process changes for 5 seconds (demo)
    let timeout = tokio::time::sleep(Duration::from_secs(5));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            Ok(change) = rx.recv() => {
                println!("   📝 Change detected: {:?}", change.path);
                println!("      Kind: {:?}, Source: {:?}", change.kind, change.source);
            }
            _ = &mut timeout => {
                println!("   (Demo timeout reached)");
                break;
            }
        }
    }

    watch_handle.abort();
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 DX Forge - Orchestration Engine Demo\n");
    println!("   This example demonstrates tool coordination with:");
    println!("   - Priority-based execution order");
    println!("   - Dependency resolution");
    println!("   - Traffic branch safety logic");
    println!("   - Dual-watcher change detection\n");

    // Get repository path
    let repo_path = std::env::current_dir()
        .context("Failed to get current directory")?
        .to_str()
        .context("Invalid UTF-8 in path")?
        .to_string();

    // Create orchestrator
    println!("⚙️  Initializing Orchestrator...");
    let mut orchestrator = Orchestrator::new(&repo_path)?;

    // Register tools
    println!("   Registering DX tools:");
    orchestrator.register_tool(Box::new(DxStyleTool));
    println!("   ✓ dx-style (priority: 100)");

    orchestrator.register_tool(Box::new(DxUiTool));
    println!("   ✓ dx-ui (priority: 80, depends: dx-style)");

    orchestrator.register_tool(Box::new(DxIconsTool));
    println!("   ✓ dx-icons (priority: 60, depends: dx-ui)");

    orchestrator.register_tool(Box::new(DxCheckTool));
    println!("   ✓ dx-check (priority: 10, depends: dx-style, dx-ui, dx-icons)");

    // Execute all tools
    println!("\n🔄 Executing tools in dependency order...\n");
    match orchestrator.execute_all().await {
        Ok(outputs) => {
            println!("\n✨ Execution complete! Results:\n");
            for output in outputs {
                let status = if output.success { "✅" } else { "❌" };
                println!("   {} {}", status, output.message);

                if !output.warnings.is_empty() {
                    for warning in &output.warnings {
                        println!("      ⚠️  {}", warning);
                    }
                }

                if !output.errors.is_empty() {
                    for error in &output.errors {
                        println!("      ❌ {}", error);
                    }
                }

                if !output.artifacts.is_empty() {
                    println!("      📦 Artifacts:");
                    for (key, value) in &output.artifacts {
                        println!("         - {}: {}", key, value);
                    }
                }

                println!("      ⏱️  Execution time: {:?}", output.execution_time);
                println!();
            }
        }
        Err(e) => {
            eprintln!("❌ Orchestration failed: {}", e);
            return Err(e);
        }
    }

    // Demonstrate traffic branch analysis
    println!("🚦 Traffic Branch Analysis:");
    let analyzer = SimpleTrafficAnalyzer;
    let branch = analyzer.analyze_change(
        Path::new("src/components/Button.tsx"),
        "old content",
        "new content",
    );

    match branch {
        TrafficBranch::Green => println!("   🟢 Green: Safe to auto-update"),
        TrafficBranch::Yellow(conflicts) => {
            println!("   🟡 Yellow: Merge required");
            for conflict in conflicts {
                println!("      - {}", conflict);
            }
        }
        TrafficBranch::Red(conflicts) => {
            println!("   🔴 Red: Manual resolution required");
            for conflict in conflicts {
                println!("      - {}", conflict);
            }
        }
    }

    // Start file watcher (for 5 seconds)
    if let Err(e) = monitor_changes(&repo_path).await {
        eprintln!("Watcher demo failed: {}", e);
    }

    println!("\n🎉 Demo complete!\n");
    println!("Next steps:");
    println!("  - Create tool manifests in tools/ directory");
    println!("  - Implement DxTool trait for your tools");
    println!("  - Configure execution order in orchestration.toml");
    println!("  - Run: forge orchestrate --watch\n");

    Ok(())
}
