# Forge Architecture - DX Orchestration Engine

> **Version**: 1.0.0  
> **Status**: Production-Ready  
> **Purpose**: Version control + tool orchestration for DX ecosystem

---

## 🎯 Mission

Forge is the **orchestration backbone** of the DX ecosystem, providing:

1. **Version Control** - Content-addressable storage with Git interoperability
2. **Tool Orchestration** - Manage execution order and dependencies of dx-tools
3. **Smart Updates** - Traffic branch system (🟢 Green, 🟡 Yellow, 🔴 Red)
4. **LSP Integration** - Real-time change detection via Language Server Protocol
5. **Zero Node_modules** - Direct file injection, no bloated dependencies

---

## 🏗️ System Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        DX ECOSYSTEM                              │
├─────────────────────────────────────────────────────────────────┤
│  dx-style │ dx-i18n │ dx-ui │ dx-icons │ dx-fonts │ dx-check │  │
│    (CSS)  │ (Locale)│ (Comp)│ (Icons)  │ (Fonts)  │  (Lint)  │  │
└────┬──────┴────┬────┴───┬───┴────┬─────┴────┬─────┴────┬──────┘
     │           │        │        │          │          │
     └───────────┴────────┴────────┴──────────┴──────────┘
                          │
                    ┌─────▼──────┐
                    │   FORGE    │  ← Orchestration Engine
                    │  (v1.0.0)  │
                    └─────┬──────┘
                          │
        ┌─────────────────┼─────────────────┐
        │                 │                 │
   ┌────▼────┐      ┌────▼────┐      ┌────▼────┐
   │   LSP   │      │ Watcher │      │ Storage │
   │ Monitor │      │  (FS)   │      │ (.dx)   │
   └─────────┘      └─────────┘      └─────────┘
```

---

## 📦 Core Components

### 1. **Storage Layer** (`.dx/forge/`)

```
.dx/forge/
├── objects/              # Content-addressable blobs (SHA-256)
├── refs/                 # Branch pointers (Git-compatible)
├── logs/                 # Operation audit trail
├── context/              # AI discussions & annotations
├── tools/                # DX tool manifests
│   ├── dx-style.toml
│   ├── dx-ui.toml
│   ├── dx-icons.toml
│   └── orchestration.toml
├── config.json           # Repository config
└── forge.db             # SQLite: operations, anchors, tool state
```

### 2. **Dual-Watcher Architecture**

#### **LSP Watcher** (Primary)
- Monitors Language Server Protocol events
- Detects code changes before file save
- Captures semantic changes (imports, exports, symbols)
- **Protocol**: LSP `textDocument/didChange`

#### **File System Watcher** (Fallback)
- Monitors file system events (create, modify, delete)
- Backup when LSP unavailable
- Cross-platform: inotify (Linux), FSEvents (macOS), ReadDirectoryChangesW (Windows)

### 3. **Tool Orchestration Engine**

Each DX tool has a manifest:

```toml
# tools/dx-ui.toml
[tool]
name = "dx-ui"
version = "1.0.0"
priority = 10           # Execution order (lower = earlier)

[dependencies]
requires = []           # No dependencies
conflicts = []          # No conflicts

[triggers]
patterns = ["dx*"]      # Trigger on "dx<ComponentName>"
extensions = [".tsx", ".jsx", ".ts", ".js"]

[traffic_branch]
enabled = true
strategy = "smart"      # green/yellow/red logic

[execution]
order = "pre-commit"    # When to run
timeout = 5000          # 5 seconds max
```

### 4. **Traffic Branch System**

```rust
pub enum TrafficBranch {
    Green {                    // 🟢 Auto-update safe
        reason: "No local changes"
    },
    Yellow {                   // 🟡 Can merge
        conflicts: Vec<Conflict>,
        strategy: MergeStrategy
    },
    Red {                      // 🔴 Manual resolution
        conflicts: Vec<Conflict>,
        blocking: true
    }
}
```

**Update Flow**:
1. User writes `dxButton` in code
2. Forge detects via LSP
3. Check component in `.dx/forge/tools/dx-ui/Button.tsx`
4. Calculate SHA-256 hash
5. Compare with base_hash
6. Determine traffic branch
7. Apply update strategy

---

## 🔄 Tool Execution Order

```toml
# orchestration.toml
[execution_order]
# Phase 1: Pre-processing
pre = [
    "dx-check",      # Format/lint first
    "dx-i18n"        # Generate locales
]

# Phase 2: Main processing
main = [
    "dx-style",      # Generate CSS
    "dx-fonts",      # Inject fonts
    "dx-icons"       # Inject icons
]

# Phase 3: Component injection
components = [
    "dx-ui",         # Inject UI components
    "dx-auth"        # Inject auth files
]

# Phase 4: Post-processing
post = [
    "dx-check"       # Final format/lint
]
```

---

## 🚀 API Surface (Public Crate)

### Core Traits

```rust
/// Main orchestrator trait
pub trait DxTool {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn execute(&mut self, context: &ExecutionContext) -> Result<ToolOutput>;
    fn priority(&self) -> u32;
}

/// File change detection
pub trait ChangeDetector {
    fn watch(&mut self, path: &Path) -> Result<ChangeStream>;
    fn stop(&mut self) -> Result<()>;
}

/// Traffic branch analysis
pub trait TrafficAnalyzer {
    fn analyze(&self, file: &Path) -> Result<TrafficBranch>;
    fn can_auto_merge(&self, conflicts: &[Conflict]) -> bool;
}
```

### Public API

```rust
// In lib.rs
pub mod orchestrator;
pub mod watcher;
pub mod storage;
pub mod traffic_branch;
pub mod tools;

pub use orchestrator::Orchestrator;
pub use watcher::{LspWatcher, FileWatcher, DualWatcher};
pub use traffic_branch::{TrafficBranch, TrafficAnalyzer};
pub use tools::{DxTool, ToolManifest, ExecutionOrder};
```

---

## 📊 Performance Targets

| Metric | Target | Current |
|--------|--------|---------|
| File change detection | < 10ms | TBD |
| Blob storage/retrieval | < 5ms | TBD |
| Tool execution | < 100ms | TBD |
| Memory footprint | < 50MB | TBD |
| Database queries | < 1ms | TBD |

---

## 🔐 Security Model

1. **Content Verification**: SHA-256 hashing prevents tampering
2. **Sandboxed Execution**: Tools run in isolated contexts
3. **Permission Model**: Explicit file access grants
4. **Audit Trail**: All operations logged with timestamps
5. **Signature Verification**: Tool manifests signed with ed25519

---

## 🌐 R2 Storage Integration

```rust
pub struct R2SyncEngine {
    local: PathBuf,           // .dx/forge/objects/
    remote: R2Storage,        // Cloudflare R2
    sync_interval: Duration,  // 5 minutes
}

impl R2SyncEngine {
    pub async fn sync_up(&self) -> Result<usize> {
        // Upload new blobs to R2
    }
    
    pub async fn sync_down(&self) -> Result<usize> {
        // Fetch missing blobs from R2
    }
}
```

**Benefits**:
- Zero egress fees (Cloudflare R2)
- Automatic backup
- Cross-machine sync
- Team collaboration

---

## 📝 Example Usage

### In a DX Tool (dx-ui)

```rust
use forge::{Orchestrator, DxTool, ExecutionContext};

pub struct DxUi {
    version: String,
}

impl DxTool for DxUi {
    fn name(&self) -> &str { "dx-ui" }
    
    fn version(&self) -> &str { &self.version }
    
    fn execute(&mut self, ctx: &ExecutionContext) -> Result<ToolOutput> {
        // 1. Detect `dx*` patterns in file
        let patterns = ctx.find_patterns(r"dx([A-Z][a-zA-Z]+)")?;
        
        // 2. For each pattern, inject component
        for pattern in patterns {
            let component = pattern.captures[0];
            
            // 3. Check traffic branch
            let traffic = ctx.traffic_analyzer.analyze(&component)?;
            
            // 4. Apply based on traffic
            match traffic {
                TrafficBranch::Green => {
                    self.inject_component(&component, ctx)?;
                }
                TrafficBranch::Yellow { conflicts, .. } => {
                    self.merge_component(&component, conflicts, ctx)?;
                }
                TrafficBranch::Red { .. } => {
                    return Err(anyhow!("Manual resolution required"));
                }
            }
        }
        
        Ok(ToolOutput::success())
    }
    
    fn priority(&self) -> u32 { 10 }
}
```

---

## 🎯 Next Steps

1. ✅ Implement `Orchestrator` core
2. ✅ Implement `DualWatcher` (LSP + FS)
3. ✅ Implement `ToolManifest` parser
4. ✅ Implement `TrafficAnalyzer` 
5. ✅ Implement `R2SyncEngine`
6. ✅ Create CLI interface
7. ✅ Publish to crates.io

---

## 📄 License

MIT + Apache 2.0 (dual-licensed for maximum compatibility)
