# Simplified Architecture: Self-Contained Tools

## Core Philosophy

**Forge is a dumb coordinator. Tools are smart and autonomous.**

## What Forge Does (Simple)

1. **Detect Changes**: Via LSP + File System watcher
2. **Ask Tools**: "Hey, something changed. Should you run?"
3. **Execute**: Run tools in priority order if they say yes
4. **Track**: Store results in blob storage

That's it. Forge doesn't know about:
- What files tools process
- What patterns tools detect
- How tools inject code
- Where tools store their data

## What Tools Do (Smart)

Each DX tool is a complete, self-contained package that knows:

```rust
pub trait DxTool {
    fn name(&self) -> &str;           // "dx-ui"
    fn version(&self) -> &str;        // "2.1.0"
    fn priority(&self) -> u32;        // 80 (lower runs first)
    
    // Tool decides if it should run based on what changed
    fn should_run(&self, ctx: &ExecutionContext) -> bool {
        // Example: dx-ui only runs if .tsx/.jsx files changed
        ctx.changed_files.iter().any(|f| {
            f.extension()
                .and_then(|e| e.to_str())
                .map(|e| e == "tsx" || e == "jsx")
                .unwrap_or(false)
        })
    }
    
    // Tool does its work
    fn execute(&mut self, ctx: &ExecutionContext) -> Result<ToolOutput>;
}
```

## Example: How dx-ui Works

### Without Forge (Traditional)
```bash
npm install @dx/ui  # 50MB of dependencies
npm install react   # Another 20MB
npm install ...     # 500MB total node_modules
```

### With Forge (Zero-Bloat)

1. **User types**: `<dxButton>Click me</dxButton>`
2. **LSP detects**: "dxButton" pattern in file
3. **Forge asks dx-ui**: "Should you run?"
4. **dx-ui responds**: "Yes! I found dxButton"
5. **dx-ui executes**:
   - Fetches Button.tsx from R2 (SHA-256: a7f3b9c8...)
   - Injects: `import { dxButton } from '.dx/cache/dx-ui/Button.tsx'`
   - Stores blob locally
6. **Result**: Only Button.tsx downloaded (~5KB vs 50MB)

## Tool Registration

Tools register themselves with Forge on startup:

```rust
fn main() -> Result<()> {
    let mut orchestrator = Orchestrator::new(".")?;
    
    // Each tool configures itself
    orchestrator.register_tool(Box::new(DxStyleTool::new()))?;   // Priority: 100
    orchestrator.register_tool(Box::new(DxUiTool::new()))?;      // Priority: 80
    orchestrator.register_tool(Box::new(DxIconsTool::new()))?;   // Priority: 70
    
    // Forge just runs them when changes are detected
    orchestrator.execute_all()?;
    Ok(())
}
```

## Configuration Files

### orchestration.toml (Simple)

```toml
[orchestration]
version = "1.0"
parallel_execution = false
fail_fast = true

[traffic]
enabled = true
# Tools decide their own traffic rules

[watcher]
enabled = true
debounce_ms = 100

[storage]
blob_dir = ".dx/forge/objects"
r2_bucket = "dx-forge-production"

[settings]
max_concurrent_tools = 4
tool_timeout_seconds = 60
```

**No tool manifests. No per-tool configuration. Tools are autonomous.**

## Execution Flow

```
1. File Change Detected
   ↓
2. Forge: "Something changed in Button.tsx"
   ↓
3. Forge asks dx-style: "Should you run?"
   dx-style: "No, not a CSS file"
   ↓
4. Forge asks dx-ui: "Should you run?"
   dx-ui: "Yes! It's a .tsx file and I see dxButton"
   ↓
5. Forge: "OK, execute dx-ui"
   ↓
6. dx-ui: 
   - Detects dxButton pattern
   - Fetches from R2
   - Injects import
   - Returns success
   ↓
7. Forge: "Done!"
```

## Traffic Branch Safety

Tools report their traffic status:

```rust
impl DxTool for DxUiTool {
    fn execute(&mut self, ctx: &ExecutionContext) -> Result<ToolOutput> {
        // Tool analyzes changes
        let traffic = self.analyze_change(file)?;
        
        match traffic {
            TrafficBranch::Green => {
                // Safe change - inject automatically
                self.inject_component()?;
            }
            TrafficBranch::Yellow { conflicts } => {
                // Potential conflict - ask user
                if user_confirms()? {
                    self.inject_with_merge()?;
                }
            }
            TrafficBranch::Red { conflicts } => {
                // Breaking change - require manual fix
                return Err(anyhow!("Manual resolution required"));
            }
        }
        
        Ok(ToolOutput::success())
    }
}
```

## Why This Approach?

### ❌ Old Way (Complex Orchestration)
- Forge knows about every tool's triggers
- Manifest files for each tool
- Central configuration management
- Tools are dumb, orchestrator is smart
- **400+ lines of configuration**

### ✅ New Way (Self-Contained Tools)
- Tools know their own business
- Zero manifest files
- Minimal configuration
- Tools are smart, orchestrator is dumb
- **20 lines of configuration**

## Real-World Example

### Traditional npm approach:
```bash
npm install @dx/ui @dx/style @dx/icons @dx/fonts
# Result: 500MB node_modules, 50,000 files
```

### Forge approach:
```bash
forge register dx-ui dx-style dx-icons dx-fonts
# Result: Tools registered, 0 bytes downloaded

# User types: <dxButton><dxiCheck />Click</dxButton>
# Forge detects change → asks tools → they download only:
# - Button.tsx (5KB)
# - button.css (2KB)  
# - Check.svg (1KB)
# Total: 8KB vs 500MB
```

## Summary

**Forge's job**: Detect changes and say "Go!"

**Tool's job**: Everything else

This keeps Forge simple, tools autonomous, and the entire ecosystem loosely coupled.

---

**Architecture Principle**: Dumb coordinator + Smart tools = Zero bloat + Maximum flexibility
