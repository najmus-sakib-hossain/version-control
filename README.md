# Forge

Operation-level version control powered by CRDTs. Forge tracks file edits as fine-grained operations, persists them in a local DeltaDB, and keeps peers in sync over WebSockets.

## ⚡ Performance

- **Sub-20µs** operation processing (dx-style inspired)
- **notify-debouncer-full** architecture eliminates Windows atomic save noise
- **3 watch modes**: polling, raw (5ms), debounced (default 3ms)
- See [DEBOUNCER_QUICKSTART.md](./DEBOUNCER_QUICKSTART.md) for configuration

## Quick Start

```bash
# Default mode (3ms debounce, recommended)
cargo run --release

# Ultra-fast mode (1ms debounce)
DX_DEBOUNCE_MS=1 cargo run --release

# Raw mode (5ms minimum gap, fastest latency)
DX_WATCH_RAW=1 cargo run --release

# Enable profiling
DX_WATCH_PROFILE=1 cargo run --release
```

## Documentation

- [DEBOUNCER_QUICKSTART.md](./DEBOUNCER_QUICKSTART.md) - Quick reference
- [DEBOUNCER_IMPLEMENTATION.md](./DEBOUNCER_IMPLEMENTATION.md) - Technical details
- [DEBOUNCER_COMPLETE.md](./DEBOUNCER_COMPLETE.md) - Implementation summary

What do you mean?? I don't understand the question.
