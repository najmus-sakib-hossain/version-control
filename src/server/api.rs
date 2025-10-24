use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::{
    Json, Router,
    extract::Query,
    extract::State,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    routing::get,
};
use colored::*;
use futures::{SinkExt, StreamExt};

use crate::crdt::Operation;
use crate::storage::{Database, OperationLog};
use crate::sync::{GLOBAL_CLOCK, SyncManager, SyncMessage};
use dashmap::DashSet;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    pub oplog: Arc<OperationLog>,
    pub db: Arc<Database>,
    pub sync: SyncManager,
    pub actor_id: String,
    pub repo_id: String,
    pub seen: Arc<DashSet<Uuid>>,
}

pub async fn serve(port: u16, path: PathBuf) -> Result<()> {
    // Initialize DB/oplog
    let forge_path = path.join(".dx/forge");
    let db = Arc::new(Database::new(&forge_path)?);
    db.initialize()?;
    let oplog = Arc::new(OperationLog::new(db.clone()));

    // Load actor/repo identifiers
    let config_path = forge_path.join("config.json");
    let default_repo_id = {
        let mut hasher = Sha256::new();
        let path_string = forge_path.to_string_lossy().into_owned();
        hasher.update(path_string.as_bytes());
        format!("repo-{:x}", hasher.finalize())
    };

    let (actor_id, repo_id) = if let Ok(bytes) = tokio::fs::read(&config_path).await {
        if let Ok(cfg) = serde_json::from_slice::<serde_json::Value>(&bytes) {
            let actor = cfg
                .get("actor_id")
                .and_then(|s| s.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| whoami::username());
            let repo = cfg
                .get("repo_id")
                .and_then(|s| s.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| default_repo_id.clone());
            (actor, repo)
        } else {
            (whoami::username(), default_repo_id.clone())
        }
    } else {
        (whoami::username(), default_repo_id)
    };

    let state = AppState {
        oplog,
        db,
        sync: SyncManager::new(),
        actor_id,
        repo_id,
        seen: Arc::new(DashSet::new()),
    };

    let app = Router::new()
        .route("/", get(|| async { "Forge DeltaDB Server" }))
        .route("/health", get(|| async { Json("OK") }))
        .route("/ops", get(get_ops))
        .route("/ws", get(ws_handler))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    println!(
        "{} Server running at {}",
        "✓".green(),
        format!("http://{}", addr).bright_blue()
    );

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn ws_handler(
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> impl axum::response::IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(state, socket))
}

async fn handle_ws(state: AppState, socket: WebSocket) {
    let (mut sender, mut receiver) = socket.split();

    // Send handshake immediately with server metadata
    let handshake = SyncMessage::handshake(state.actor_id.clone(), state.repo_id.clone());
    if let Ok(text) = serde_json::to_string(&handshake) {
        let _ = sender.send(Message::Text(text.into())).await;
    }

    // Subscribe to local operations and forward to this client
    let mut rx = state.sync.subscribe();
    let send_task = tokio::spawn(async move {
        while let Ok(op_arc) = rx.recv().await {
            // Forward as JSON text
            if let Ok(text) = serde_json::to_string(&SyncMessage::operation((*op_arc).clone())) {
                if sender.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
        }
    });

    // Receive from client and publish
    let state_recv = state.clone();
    let recv_task = tokio::spawn(async move {
        let oplog = state_recv.oplog.clone();
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    let text: String = text.to_string();
                    if let Ok(msg) = serde_json::from_str::<SyncMessage>(&text) {
                        match msg {
                            SyncMessage::Handshake { actor_id, repo_id } => {
                                println!(
                                    "{} Peer handshake: actor={} repo={}",
                                    "↔".bright_blue(),
                                    actor_id.bright_yellow(),
                                    repo_id.bright_white()
                                );
                            }
                            SyncMessage::Operation { operation: op } => {
                                if op.actor_id != state_recv.actor_id
                                    && state_recv.seen.insert(op.id)
                                {
                                    if let Some(lamport) = op.lamport() {
                                        GLOBAL_CLOCK.observe(lamport);
                                    }
                                    if let Ok(true) = oplog.append(op.clone()).await {
                                        let _ = state_recv.sync.publish(Arc::new(op));
                                    }
                                }
                            }
                        }
                    } else if let Ok(op) = serde_json::from_str::<Operation>(&text) {
                        if op.actor_id != state_recv.actor_id && state_recv.seen.insert(op.id) {
                            if let Some(lamport) = op.lamport() {
                                GLOBAL_CLOCK.observe(lamport);
                            }
                            if let Ok(true) = oplog.append(op.clone()).await {
                                let _ = state_recv.sync.publish(Arc::new(op));
                            }
                        }
                    }
                }
                Ok(Message::Binary(bin)) => {
                    if let Ok(op) = serde_cbor::from_slice::<Operation>(&bin) {
                        if op.actor_id != state_recv.actor_id && state_recv.seen.insert(op.id) {
                            if let Some(lamport) = op.lamport() {
                                GLOBAL_CLOCK.observe(lamport);
                            }
                            if let Ok(true) = oplog.append(op.clone()).await {
                                let _ = state_recv.sync.publish(Arc::new(op));
                            }
                        }
                    }
                }
                Ok(Message::Close(_)) | Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {}
                Err(_) => break,
            }
        }
    });

    let _ = tokio::join!(send_task, recv_task);
}

#[derive(Deserialize)]
struct OpsQuery {
    file: Option<String>,
    limit: Option<usize>,
}

async fn get_ops(
    State(state): State<AppState>,
    Query(query): Query<OpsQuery>,
) -> Result<Json<Vec<Operation>>, axum::http::StatusCode> {
    let limit = query.limit.unwrap_or(50);
    let result = if let Some(file) = query.file.as_deref() {
        let p = std::path::PathBuf::from(file);
        state.db.get_operations(Some(&p), limit)
    } else {
        state.db.get_operations(None, limit)
    };

    match result {
        Ok(ops) => Ok(Json(ops)),
        Err(_) => Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR),
    }
}
