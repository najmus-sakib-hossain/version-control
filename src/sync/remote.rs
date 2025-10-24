use std::sync::Arc;

use anyhow::{anyhow, Result};
use futures::{SinkExt, StreamExt};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use url::Url;

use crate::crdt::Operation;
use crate::storage::OperationLog;
use super::protocol::SyncManager;
use once_cell::sync::Lazy;
use dashmap::DashSet;
use uuid::Uuid;

/// Connect to a remote WebSocket peer and bridge operations between the
/// in-process SyncManager and the remote. Returns a JoinHandle for the
/// background task managing the connection.
pub async fn connect_peer(
    url: &str,
    actor_id: String,
    sync: SyncManager,
    oplog: Arc<OperationLog>,
) -> Result<JoinHandle<()>> {
    let url = Url::parse(url).map_err(|e| anyhow!("invalid ws url: {e}"))?;
    let (ws_stream, _) = tokio_tungstenite::connect_async(url.as_str()).await?;

    let (mut ws_tx, mut ws_rx) = ws_stream.split();

    // Subscribe to local ops to forward to remote
    let mut rx = sync.subscribe();

    // Spawn forwarder for local -> remote
    let actor_id_clone = actor_id.clone();
    let forward = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(op_arc) => {
                    // Only forward our own actor's ops to reduce echo, server will broadcast
                    if op_arc.actor_id == actor_id_clone && SEEN.insert(op_arc.id) {
                        if let Ok(json) = serde_json::to_string(&*op_arc) {
                            if ws_tx.send(Message::Text(json.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Spawn receiver for remote -> local
    let sync_clone = sync.clone();
    let actor_id_clone2 = actor_id.clone();
    let oplog_clone = oplog.clone();
    let recv = tokio::spawn(async move {
        while let Some(msg) = ws_rx.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    let text: String = text.to_string();
                    if let Ok(op) = serde_json::from_str::<Operation>(&text) {
                        // Drop our own echoes
                        if op.actor_id != actor_id_clone2 && SEEN.insert(op.id) {
                            // Append to log and publish to local subscribers
                            let _ = oplog_clone.append(op.clone()).await;
                            let _ = sync_clone.publish(Arc::new(op));
                        }
                    }
                }
                Ok(Message::Binary(bin)) => {
                    if let Ok(op) = serde_cbor::from_slice::<Operation>(&bin) {
                        if op.actor_id != actor_id_clone2 && SEEN.insert(op.id) {
                            let _ = oplog_clone.append(op.clone()).await;
                            let _ = sync_clone.publish(Arc::new(op));
                        }
                    }
                }
                Ok(Message::Frame(_)) => { /* ignore */ }
                Ok(Message::Close(_)) | Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {
                    // no-op
                }
                Err(_) => break,
            }
        }
    });

    // Join both tasks under a single handle
    let handle = tokio::spawn(async move {
        let _ = tokio::join!(forward, recv);
    });

    Ok(handle)
}

static SEEN: Lazy<DashSet<Uuid>> = Lazy::new(|| DashSet::new());
