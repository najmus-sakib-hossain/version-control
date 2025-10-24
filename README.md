# Forge

Operation-level version control powered by CRDTs. Forge tracks file edits as fine-grained operations, persists them in a local DeltaDB, and keeps peers in sync over WebSockets.

## Getting Started

```bash
cargo run -- init
cargo run -- watch
```

Use `cargo run -- help` (or `forge help` when installed) to explore the complete CLI.

## Sync Implementation Notes

- Cold-start sync from a peer is handled by issuing a GET `:/ops` snapshot before switching to WebSocket live updates. `connect_peer` automatically wires this initial fetch so late joiners catch up.
- De-dup helps in multi-peer topologies; for full causal consistency, we could add vector clocks or use causal dependencies via `parent_ops`.
- Recent improvements:
  1. Added a lightweight handshake message on WebSocket connect (actor id and repo fingerprint).
  2. Implemented an initial `/ops` sync in `connect_peer` so replicas hydrate before subscribing to real-time traffic.
  3. Hardened the Lamport clock across processes with a hybrid logical clock and remote observation.
