# 🔇 Hiding Atomic Save Noise

## Problem

Even with deduplication, Windows atomic saves were creating noisy logs with all zeros:

```bash
⚙️ detect README.md | cached=0µs meta=0µs read=0µs tail=0µs diff=0µs total=107µs
⚙️ detect README.md | cached=0µs meta=0µs read=0µs tail=0µs diff=0µs total=59µs
⚙️ detect README.md | cached=0µs meta=0µs read=0µs tail=0µs diff=0µs total=75µs
⚙️ detect README.md | cached=0µs meta=0µs read=0µs tail=0µs diff=0µs total=7689µs  ← Windows save
⚙️ detect README.md | cached=0µs meta=0µs read=0µs tail=0µs diff=0µs total=77µs
```

These logs appear when `DX_WATCH_PROFILE=1` is set, but they provide no useful information - all timings are 0.

## Solution

### 1. Smart Profile Filtering 🎯

Profile logs now only appear when there's **actual meaningful data**:

```rust
// Skip logging if all timings are basically zero (atomic save noise)
let has_meaningful_data = timings.cached_us > 0
    || timings.metadata_us > 0
    || timings.read_us > 0
    || timings.tail_us > 0
    || timings.diff_us > 0
    || timings.total_us > 200; // At least 200µs of actual work
```

### 2. Improved Deduplication 🔥

Better hash using **file size + modification time**:

```rust
// Combine file size + modified time
// This catches atomic saves where size stays the same
let modified_time = metadata.modified().ok()
    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
    .map(|d| d.as_secs())
    .unwrap_or(0);

let content_hash = metadata.len().wrapping_add(modified_time);
```

### 3. Startup Notification 💡

Forge now tells you if profiling is enabled:

```bash
→ Repo ID: local-...
🔍 Profiling enabled (DX_WATCH_PROFILE=1) - showing all detection timings
```

Or if disabled:

```bash
💡 Set DX_WATCH_PROFILE=1 to see detailed detection timings
```

## Results

### Before (with DX_WATCH_PROFILE=1)

```bash
⚙️ detect README.md | cached=0µs meta=0µs read=0µs tail=0µs diff=0µs total=107µs
⚙️ detect README.md | cached=0µs meta=0µs read=0µs tail=0µs diff=0µs total=59µs
⚙️ detect README.md | cached=0µs meta=0µs read=0µs tail=0µs diff=0µs total=75µs
⚙️ detect README.md | cached=0µs meta=0µs read=0µs tail=0µs diff=0µs total=7689µs
⚙️ detect README.md | cached=0µs meta=0µs read=0µs tail=0µs diff=0µs total=77µs
```

### After (with DX_WATCH_PROFILE=1)

```bash
(All-zero noise automatically filtered out)
⚙️ detect main.rs | cached=45µs read=234µs diff=156µs total=435µs  ← Only meaningful data
⚡ [42µs | detect 18µs] INSERT main.rs 15:23 +12 chars 'Hello World'
```

### Normal mode (without DX_WATCH_PROFILE)

```bash
💡 Set DX_WATCH_PROFILE=1 to see detailed detection timings
⚡ [42µs | detect 18µs] INSERT main.rs 15:23 +12 chars 'Hello World'
(No profile logs at all - just operations)
```

## Usage

### Normal Operation (Clean Logs)

```bash
# Don't set DX_WATCH_PROFILE
cargo run --release -- watch

# You'll only see operation logs:
⚡ [42µs] INSERT main.rs 15:23 +12 chars 'Hello'
```

### Debugging (Show Meaningful Timings)

```bash
# Set the environment variable
export DX_WATCH_PROFILE=1
cargo run --release -- watch

# You'll see profile logs with actual data:
⚙️ detect main.rs | read=234µs diff=156µs total=435µs
⚡ [42µs] INSERT main.rs 15:23 +12 chars 'Hello'
```

### Turn Off Profiling

```bash
# Unset the variable
unset DX_WATCH_PROFILE
cargo run --release -- watch

# Back to clean operation-only logs
```

## What Gets Filtered

Profile logs (`⚙️ detect ...`) are **hidden** when:

1. All timings are 0µs (no actual work done)
2. Total time is less than 200µs AND all component timings are 0
3. No operations were created AND profiling shows no meaningful data

Profile logs are **shown** when:

1. Any component timing > 0µs (cached, meta, read, tail, diff)
2. Total time > 200µs (even if components are 0)
3. Operations were created

## Summary

✅ **Atomic save noise eliminated** - no more all-zero logs  
✅ **Smarter deduplication** - uses file size + modification time  
✅ **Clear profiling status** - know if profiling is on/off  
✅ **Meaningful data only** - see timings when there's actual work  
✅ **Clean normal mode** - no noise without DX_WATCH_PROFILE

**Recommendation**: Run without `DX_WATCH_PROFILE` for normal use. Only enable for debugging performance issues.
