# Forge - Ultra-Fast CRDT Version Control

Operation-level version control powered by CRDTs. Forge tracks file edits as fine-grained operations, persists them in a local DeltaDB, and keeps peers in sync over WebSockets.

## ⚡⚡ Dual-Watcher Architecture

Forge uses a **dual-watcher system** for maximum performance and quality:

### 🚀 Mode 1: RAPID Detection (<20µs)

- **Zero syscalls** - Uses atomic sequence counter (no time calls!)
- **No file operations** - Skips metadata, mtime, and content reads
- **Instant feedback** - Ultra-fast change logging
- **Target: <20µs** - Ultra-fast notification system

### 📊 Mode 2: QUALITY Detection (<60µs)

- **Full file analysis** - Complete operation detection with line numbers
- **Rich metadata** - Diffs, timestamps, and sync details
- **Background execution** - Runs after rapid mode
- **Target: <60µs** - Fast detailed analysis

Both modes run sequentially for every file change, providing instant feedback (rapid) followed by complete details (quality).

## 🎯 Performance Targets

- **RAPID mode**: <20µs change detection ✅ **ACHIEVED: 3-20µs**
- **QUALITY mode**: <100µs operation detection ⚠️ **CURRENT: ~60-300µs**
- **Total latency**: <320µs for complete processing
- **Debounce**: 1ms ultra-fast mode
- **Inspired by**: dx-style project's <100µs techniques

### Current Performance

```bash
# Small appends (cached, best case)
⚡ [RAPID 3µs] test.txt changed
✨ [QUALITY 58µs | total 61µs] test.txt - 1 ops

# Regular edits (typical case)  
⚡ [RAPID 20µs] test.txt changed
🐢 [QUALITY 301µs | total 321µs] test.txt - 1 ops
```

**RAPID mode**: ✅ Target exceeded (3µs is 6x faster than 20µs goal!)
**QUALITY mode**: ⚠️ 58-301µs (varies by edit type - appends are fast, full diffs slower)

## Quick Start

```bash
# Default mode (dual-watcher enabled)
cargo run --release

# Enable profiling to see timings
DX_WATCH_PROFILE=1 cargo run --release

# Disable rapid mode (quality only, for testing)
DX_DISABLE_RAPID_MODE=1 cargo run --release

# Example output:
# ⚡ [RAPID 8µs] README.md changed
# ✨ [QUALITY 52µs | total 60µs] README.md - 1 ops
```

## Configuration

### Environment Variables

- `DX_WATCH_PROFILE=1` - Show detailed timing for both modes
- `DX_DISABLE_RAPID_MODE=1` - Disable rapid mode (quality only)
- `DX_DEBOUNCE_MS=1` - Debounce interval (default: 1ms)

### Performance Markers

- ⚡ RAPID mode ≤20µs (target achieved)
- 🐌 RAPID mode >20µs (needs optimization)
- ✨ QUALITY mode ≤60µs (target achieved)  
- 🐢 QUALITY mode >60µs (needs optimization)

**Clean output - only shows when there are changes!**

Testing no-op detection...
