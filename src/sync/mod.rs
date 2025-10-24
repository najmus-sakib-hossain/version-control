pub mod protocol;
pub mod remote;

pub use protocol::SyncManager;

// Real-time sync protocol: in-process broadcast-based sync manager
// Provides a publish/subscribe channel for Operations so the watcher
// and other components can broadcast live operations to subscribers
// (e.g. WebSocket handlers or other peers).