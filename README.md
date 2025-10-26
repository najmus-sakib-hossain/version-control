# Forge - Ultra-Fast CRDT Version Control

Operation-level version control powered by CRDTs. Forge tracks file edits as fine-grained operations, persists them in a local DeltaDB, and keeps peers in sync over WebSockets.

## ⚡⚡ Dual-Watcher Architecture

Forge uses a **dual-watcher system** for maximum performance and quality:

### 🚀 Mode 1: RAPID Detection (<20µs)

- **Zero syscalls** - Uses atomic sequence counter (no time calls!)
- **No file operations** - Skips metadata, mtime, and content reads
- **Instant feedback** - Logs file changes in microseconds
- **Target: <20µs** - Ultra-fast change notification

### 📊 Mode 2: QUALITY Detection (<60µs)

- **Full file analysis** - Complete operation detection with line numbers
- **Rich metadata** - Diffs, timestamps, and sync details
- **Background execution** - Runs after rapid mode
- **Target: <60µs** - Fast detailed analysis

Both modes run sequentially for every file change, providing instant feedback (rapid) followed by complete details (quality).

## 🎯 Performance Targets

- **RAPID mode**: <20µs change detection (no syscalls!)
- **QUALITY mode**: <60µs operation detection (full analysis)
- **Total latency**: <80µs for complete processing
- **Debounce**: 1ms ultra-fast mode
- **Inspired by**: dx-style project's <100µs techniques

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

## Documentation

- [DEBOUNCER_QUICKSTART.md](./DEBOUNCER_QUICKSTART.md) - Quick reference
- [DEBOUNCER_IMPLEMENTATION.md](./DEBOUNCER_IMPLEMENTATION.md) - Technical details
- [DEBOUNCER_COMPLETE.md](./DEBOUNCER_COMPLETE.md) - Implementation summary
- [PERFORMANCE.md](./PERFORMANCE.md) - Optimization techniques
