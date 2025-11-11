use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::State,
    extract::{Path as AxumPath, Query},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use colored::*;
use futures::{SinkExt, StreamExt};
use tower_http::cors::{Any, CorsLayer};

use crate::crdt::Operation;
use crate::storage::{Blob, Database, OperationLog, R2Config, R2Storage};
use crate::sync::{SyncManager, SyncMessage, GLOBAL_CLOCK};
use dashmap::DashSet;
use serde::{Deserialize, Serialize};
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
    pub r2: Option<Arc<R2Storage>>, // R2 storage for blobs
}

/// API error type
#[derive(Debug)]
pub enum ApiError {
    Internal(anyhow::Error),
    NotFound(String),
    BadRequest(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::Internal(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
        };

        let body = Json(serde_json::json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

impl<E> From<E> for ApiError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        ApiError::Internal(err.into())
    }
}

/// Blob upload request
#[derive(Debug, Deserialize)]
pub struct UploadBlobRequest {
    pub path: String,
    pub content: String, // Base64 encoded
}

/// Blob upload response
#[derive(Debug, Serialize)]
pub struct UploadBlobResponse {
    pub hash: String,
    pub key: String,
    pub size: u64,
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

    // Try to load R2 config
    let r2 = match R2Config::from_env() {
        Ok(config) => {
            println!(
                "{} R2 Bucket: {}",
                "✓".green(),
                config.bucket_name.bright_white()
            );
            match R2Storage::new(config) {
                Ok(storage) => {
                    println!("{} R2 Storage enabled", "✓".green());
                    Some(Arc::new(storage))
                }
                Err(e) => {
                    println!("{} R2 Storage failed: {}", "⚠".yellow(), e);
                    None
                }
            }
        }
        Err(_) => {
            println!(
                "{} R2 not configured (set R2_* in .env for blob storage)",
                "ℹ".blue()
            );
            None
        }
    };

    let state = AppState {
        oplog,
        db,
        sync: SyncManager::new(),
        actor_id,
        repo_id,
        seen: Arc::new(DashSet::new()),
        r2,
    };

    let app = Router::new()
        .route("/", get(|| async { "Forge API Server" }))
        .route("/health", get(health_check))
        .route("/ops", get(get_ops))
        .route("/ws", get(ws_handler))
        // Blob endpoints (if R2 is configured)
        .route("/api/v1/blobs", post(upload_blob))
        .route("/api/v1/blobs/:hash", get(download_blob))
        .route("/api/v1/blobs/:hash", delete(delete_blob_handler))
        .route("/api/v1/blobs/:hash/exists", get(check_blob_exists))
        .route("/api/v1/blobs/batch", post(batch_upload))
        // CORS for web clients
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
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
                                if insert_seen(&state_recv.seen, op.id) {
                                    if let Some(lamport) = op.lamport() {
                                        GLOBAL_CLOCK.observe(lamport);
                                    }
                                    let _ = oplog.append(op.clone());
                                    let _ = state_recv.sync.publish(Arc::new(op));
                                }
                            }
                        }
                    } else if let Ok(op) = serde_json::from_str::<Operation>(&text) {
                        if insert_seen(&state_recv.seen, op.id) {
                            if let Some(lamport) = op.lamport() {
                                GLOBAL_CLOCK.observe(lamport);
                            }
                            let _ = oplog.append(op.clone());
                            let _ = state_recv.sync.publish(Arc::new(op));
                        }
                    }
                }
                Ok(Message::Binary(bin)) => {
                    if let Ok(op) = serde_cbor::from_slice::<Operation>(&bin) {
                        if insert_seen(&state_recv.seen, op.id) {
                            if let Some(lamport) = op.lamport() {
                                GLOBAL_CLOCK.observe(lamport);
                            }
                            let _ = oplog.append(op.clone());
                            let _ = state_recv.sync.publish(Arc::new(op));
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

const SEEN_LIMIT: usize = 10_000;

fn insert_seen(cache: &DashSet<Uuid>, id: Uuid) -> bool {
    let inserted = cache.insert(id);
    if inserted {
        enforce_seen_limit(cache);
    }
    inserted
}

fn enforce_seen_limit(cache: &DashSet<Uuid>) {
    while cache.len() > SEEN_LIMIT {
        if let Some(entry) = cache.iter().next() {
            let key = *entry.key();
            drop(entry);
            cache.remove(&key);
        } else {
            break;
        }
    }
}

// ========== Blob Storage Endpoints ==========

/// Health check endpoint with R2 status
async fn health_check(State(state): State<AppState>) -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "healthy",
        "service": "forge-api",
        "version": env!("CARGO_PKG_VERSION"),
        "r2_enabled": state.r2.is_some(),
    }))
}

/// Upload blob endpoint
async fn upload_blob(
    State(state): State<AppState>,
    Json(req): Json<UploadBlobRequest>,
) -> Result<Json<UploadBlobResponse>, ApiError> {
    let r2 = state
        .r2
        .as_ref()
        .ok_or_else(|| ApiError::BadRequest("R2 storage not configured".to_string()))?;

    // Decode base64 content
    use base64::Engine;
    let content = base64::engine::general_purpose::STANDARD
        .decode(&req.content)
        .map_err(|e| ApiError::BadRequest(format!("Invalid base64: {}", e)))?;

    let blob = Blob::from_content(&req.path, content);
    let hash = blob.hash().to_string();
    let size = blob.metadata.size;

    // Upload to R2
    let key = r2.upload_blob(&blob).await?;

    Ok(Json(UploadBlobResponse { hash, key, size }))
}

/// Download blob endpoint
async fn download_blob(
    State(state): State<AppState>,
    AxumPath(hash): AxumPath<String>,
) -> Result<Response, ApiError> {
    let r2 = state
        .r2
        .as_ref()
        .ok_or_else(|| ApiError::BadRequest("R2 storage not configured".to_string()))?;

    let blob = r2
        .download_blob(&hash)
        .await
        .map_err(|_| ApiError::NotFound(format!("Blob not found: {}", hash)))?;

    // Return blob content with metadata headers
    Ok((
        StatusCode::OK,
        [
            ("Content-Type", blob.metadata.mime_type.clone()),
            ("X-Blob-Hash", hash),
            ("X-Blob-Size", blob.metadata.size.to_string()),
        ],
        blob.content,
    )
        .into_response())
}

/// Delete blob endpoint
async fn delete_blob_handler(
    State(state): State<AppState>,
    AxumPath(hash): AxumPath<String>,
) -> Result<StatusCode, ApiError> {
    let r2 = state
        .r2
        .as_ref()
        .ok_or_else(|| ApiError::BadRequest("R2 storage not configured".to_string()))?;

    r2.delete_blob(&hash).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Check if blob exists
async fn check_blob_exists(
    State(state): State<AppState>,
    AxumPath(hash): AxumPath<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let r2 = state
        .r2
        .as_ref()
        .ok_or_else(|| ApiError::BadRequest("R2 storage not configured".to_string()))?;

    let exists = r2.blob_exists(&hash).await?;

    Ok(Json(serde_json::json!({
        "exists": exists,
        "hash": hash,
    })))
}

/// Batch upload request
#[derive(Debug, Deserialize)]
pub struct BatchUploadRequest {
    pub blobs: Vec<UploadBlobRequest>,
}

/// Batch upload response
#[derive(Debug, Serialize)]
pub struct BatchUploadResponse {
    pub uploaded: Vec<UploadBlobResponse>,
    pub failed: Vec<String>,
}

/// Batch upload endpoint
async fn batch_upload(
    State(state): State<AppState>,
    Json(req): Json<BatchUploadRequest>,
) -> Result<Json<BatchUploadResponse>, ApiError> {
    let r2 = state
        .r2
        .as_ref()
        .ok_or_else(|| ApiError::BadRequest("R2 storage not configured".to_string()))?;

    let mut uploaded = Vec::new();
    let mut failed = Vec::new();

    use base64::Engine;
    for blob_req in req.blobs {
        match base64::engine::general_purpose::STANDARD.decode(&blob_req.content) {
            Ok(content) => {
                let blob = Blob::from_content(&blob_req.path, content);
                let hash = blob.hash().to_string();
                let size = blob.metadata.size;

                match r2.upload_blob(&blob).await {
                    Ok(key) => {
                        uploaded.push(UploadBlobResponse { hash, key, size });
                    }
                    Err(e) => {
                        failed.push(format!("{}: {}", blob_req.path, e));
                    }
                }
            }
            Err(e) => {
                failed.push(format!("{}: Invalid base64: {}", blob_req.path, e));
            }
        }
    }

    Ok(Json(BatchUploadResponse { uploaded, failed }))
}
