# Traffic Branch System & LSP Detection

## Overview

Forge implements two revolutionary features that make it the successor to Git:

1. **Traffic Branch System**: Intelligent component update strategy (🟢 Green, 🟡 Yellow, 🔴 Red)
2. **LSP Detection**: Automatic fallback between LSP-based and file-watching modes

---

## Traffic Branch System

The Traffic Branch System provides intelligent, automated updates for DX-managed components without requiring a manifest file. It uses a traffic light metaphor to categorize update strategies.

### How It Works

When you install a DX component (e.g., `Button.tsx` from `dx-ui`):

1. Forge computes a SHA-256 hash of the component's content
2. This hash is stored as the `base_hash` in `.dx/forge/component_state.json`
3. The `base_hash` serves as the reference point for all future updates

When an update is available:

1. Forge reads the current `LOCAL` content
2. Fetches the new `REMOTE` content  
3. Compares `BASE`, `LOCAL`, and `REMOTE` to determine the strategy

### The Three Branches

#### 🟢 Green Branch: Auto-Update

**Condition:** `hash(LOCAL) == base_hash`
- You haven't modified the component locally

**Action:**
- Forge automatically overwrites the local file with the remote version
- Updates the `base_hash` to the new version
- No user interaction required

**Example:**
```bash
$ forge update Button
🟢 components/Button.tsx updated to v2.0.0 (auto-updated)
```

#### 🟡 Yellow Branch: Smart Merge

**Condition:** `hash(LOCAL) != base_hash` AND no line conflicts
- You've modified the component locally
- Your changes don't conflict with remote updates

**Action:**
- Forge performs a 3-way merge
- Combines remote updates with your local modifications
- Preserves your customizations
- Notifies you to review the merge

**Example:**
```bash
$ forge update Button
🟡 components/Button.tsx updated to v2.0.0 (merged with local changes)
   → Please review merged changes
```

#### 🔴 Red Branch: Manual Resolution

**Condition:** `hash(LOCAL) != base_hash` AND line conflicts detected
- Both you and the author modified the same lines

**Action:**
- Forge does NOT modify your file
- Protects your code from data loss
- Provides detailed conflict information
- Offers tools to resolve conflicts

**Example:**
```bash
$ forge update Button
🔴 CONFLICT: components/Button.tsx v2.0.0
   │ Update conflicts with your local changes:
   │ Conflict at lines 15-20
   │ Conflict at lines 42-45
   └ Run forge resolve to resolve
```

### Component Management Commands

#### Register a Component

```bash
forge register <path> --source <source> --name <name> --version <version>
```

**Example:**
```bash
$ forge register components/Button.tsx --source dx-ui --name Button --version 1.0.0
✓ Registered Button v1.0.0 from dx-ui
   Path: components/Button.tsx
```

#### List Managed Components

```bash
forge components [--verbose]
```

**Example:**
```bash
$ forge components --verbose

📦 Managed Components
═══════════════════════════════════════════════════════════════════
● Button v1.0.0
   Source: dx-ui
   Path:   components/Button.tsx
   Hash:   a3f5b8c2e1d4f7e9
   Added:  2025-11-10 14:23:45

● Icon v2.1.0
   Source: dx-icon
   Path:   components/Icon.tsx
   Hash:   d7e4f1b2a9c5e8f3
   Added:  2025-11-10 14:25:12

────────────────────────────────────────────────────────────────────
2 components | Use --verbose for details | forge update <name> to update
```

#### Update Components

```bash
# Update specific component
forge update <component-name>

# Update all components
forge update all
```

**Example:**
```bash
$ forge update all

🔄 Updating all components...

→ Checking Button...
🟢 components/Button.tsx updated to v2.0.0 (auto-updated)

→ Checking Icon...
🟡 components/Icon.tsx updated to v3.0.0 (merged with local changes)

→ Checking Modal...
🔴 CONFLICT: components/Modal.tsx v1.5.0
   │ Update conflicts with your local changes
   └ Run forge resolve Modal
```

### State File Structure

Components are tracked in `.dx/forge/component_state.json`:

```json
{
  "components/Button.tsx": {
    "path": "components/Button.tsx",
    "base_hash": "a3f5b8c2e1d4f7e9c1a2b3c4d5e6f7g8h9i0j1k2l3m4n5o6p7q8r9s0",
    "source": "dx-ui",
    "name": "Button",
    "version": "1.0.0",
    "installed_at": "2025-11-10T14:23:45.123Z"
  }
}
```

---

## LSP Detection System

Forge automatically detects whether a DX code editor extension is available and chooses the optimal change detection method.

### 📡 LSP-Based Detection (Preferred)

When a DX editor extension is present, Forge uses the **Language Server Protocol** to receive change events directly from your editor.

**Benefits:**

- **Lower Latency**: Changes detected instantly without polling
- **Precise Tracking**: Exact character-level edits from the editor
- **Better Integration**: Works seamlessly with editor features
- **Reduced CPU Usage**: No file system scanning required

**How It Works:**

1. Editor extension sends `textDocument/didChange` events via LSP
2. Forge receives events with exact line/column positions
3. Events are converted to Forge operations
4. Operations are stored in the operation log

**Detection:**

Forge checks for LSP support in this order:

1. `DX_LSP_ENABLED` environment variable
2. LSP socket file at `$TEMP/dx-lsp.sock`
3. DX editor extension in `~/.vscode/extensions/`

**Installation:**

```bash
# VS Code
code --install-extension dx.forge-extension

# Or search "DX Forge" in the extension marketplace
```

**Output:**

```bash
$ forge watch
✓ DX editor extension detected
📡 LSP-based detection mode enabled
→ Listening for LSP events...
```

### 👁️ File Watching (Fallback)

When no LSP extension is detected, Forge uses high-performance file system watching.

**Features:**

- **Ultra-Fast**: Sub-35µs rapid change detection (typically 1-2µs)
- **Dual-Mode**: Rapid events + quality analysis with full details
- **Production-Ready**: Optimized with memory-mapped I/O and SIMD

**How It Works:**

1. File system notifier detects changes via `notify` crate
2. **Rapid Mode**: Instant notification (<35µs, typically 1-2µs)
3. **Quality Mode**: Full operation detection with line numbers (<60µs)
4. Operations stored in the operation log

**Output:**

```bash
$ forge watch
👁️  File watching mode (no LSP extension detected)
✔ Starting operation-level tracking...

⚡ [RAPID 2µs] src/main.rs changed
✨ [QUALITY 58µs | total 60µs]
  + src/main.rs @ 15:8
    console.log("Hello, world!");
```

### Automatic Mode Selection

Forge handles everything automatically:

```rust
// Internal logic (simplified)
pub async fn run(self) -> Result<()> {
    let lsp_available = detect_lsp_support().await?;
    
    if lsp_available {
        // Use LSP-based detection
        start_lsp_monitoring(...).await
    } else {
        // Fall back to file watching
        start_file_watching(...).await
    }
}
```

You don't need to configure anything—Forge chooses the best method based on your environment!

---

## Implementation Details

### Traffic Branch Algorithm

```rust
pub fn analyze_update(
    path: &Path,
    remote_content: &str,
) -> Result<TrafficBranch> {
    let base_hash = get_base_hash(path);
    let local_hash = compute_hash(&read_file(path));
    let remote_hash = compute_hash(remote_content);
    
    // 🟢 GREEN: No local modifications
    if local_hash == base_hash {
        return Ok(TrafficBranch::Green);
    }
    
    // Detect conflicts between local and remote
    let conflicts = detect_conflicts(
        &read_file(path),
        remote_content,
    );
    
    if conflicts.is_empty() {
        // 🟡 YELLOW: Non-conflicting changes
        Ok(TrafficBranch::Yellow { conflicts: vec![] })
    } else {
        // 🔴 RED: Conflicting changes
        Ok(TrafficBranch::Red { conflicts })
    }
}
```

### LSP Event Processing

```rust
pub async fn process_change(&self, event: LspChangeEvent) -> Result<()> {
    // Convert URI to path
    let path = uri_to_path(&event.uri)?;
    
    // Convert LSP changes to Forge operations
    let operations = convert_changes_to_operations(&path, &event.changes)?;
    
    // Store operations
    for op in operations {
        self.oplog.append(op)?;
        
        // Publish to sync if enabled
        if let Some(mgr) = &self.sync_mgr {
            mgr.publish(Arc::new(op))?;
        }
    }
    
    Ok(())
}
```

---

## Examples

### Traffic Branch Demo

Run the comprehensive demo:

```bash
cargo run --example traffic_branch_and_lsp
```

This demonstrates:
- 🟢 Green branch (auto-update)
- 🟡 Yellow branch (merge)
- 🔴 Red branch (conflict)
- LSP detection and fallback

### Real-World Usage

```bash
# 1. Initialize repository
forge init my-project
cd my-project

# 2. Register components
forge register components/Button.tsx --source dx-ui --name Button --version 1.0.0
forge register components/Icon.tsx --source dx-icon --name Icon --version 2.1.0

# 3. Start watching (automatically detects LSP or falls back to file watching)
forge watch

# 4. Update components
forge update all
```

---

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      Forge Core                             │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌──────────────────┐         ┌──────────────────┐        │
│  │ Traffic Branch   │         │ LSP Detection    │        │
│  │ System           │         │ System           │        │
│  └────────┬─────────┘         └────────┬─────────┘        │
│           │                             │                   │
│           ▼                             ▼                   │
│  ┌─────────────────────────────────────────────────┐      │
│  │        Component State Manager                   │      │
│  │  .dx/forge/component_state.json                 │      │
│  └─────────────────────────────────────────────────┘      │
│                                                             │
│  ┌──────────────┐    ┌──────────────┐   ┌──────────────┐ │
│  │ LSP Detector │───▶│ File Watcher │──▶│ Operation    │ │
│  │              │    │              │   │ Log          │ │
│  └──────────────┘    └──────────────┘   └──────────────┘ │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

---

## Benefits

### For Developers

1. **Zero Manual Merges**: Most updates happen automatically
2. **Protected Customizations**: Your changes are never lost
3. **Clear Conflict Resolution**: Exact line numbers when conflicts occur
4. **Optimal Performance**: Automatic detection mode selection

### For Teams

1. **Consistent Dependencies**: Everyone gets the same component versions
2. **Safe Updates**: Red branch protects against breaking changes
3. **Version Transparency**: Clear tracking of what's installed and modified

### For Component Authors

1. **Easier Distribution**: No manual version management
2. **Update Analytics**: Track which versions are in use
3. **Breaking Change Safety**: Red branch prevents silent failures

---

## Future Enhancements

- [ ] AI-assisted conflict resolution for Red branch
- [ ] Visual diff viewer for Yellow branch merges
- [ ] Automated testing before Green branch auto-updates
- [ ] Component dependency graph visualization
- [ ] Rollback mechanism for problematic updates
- [ ] LSP extension for more editors (Vim, Emacs, Sublime)

---

## Learn More

- [Main README](../README.md) - Forge overview
- [Examples](../examples/) - More code examples
- [API Documentation](https://docs.rs/dx-forge) - Full API reference
