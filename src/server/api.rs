use anyhow::Result;
use axum::{
    routing::get,
    Router,
    Json,
};
use std::path::PathBuf;
use colored::*;

pub async fn serve(port: u16, _path: PathBuf) -> Result<()> {
    let app = Router::new()
        .route("/", get(|| async { "Forge DeltaDB Server" }))
        .route("/health", get(|| async { Json("OK") }));

    let addr = format!("0.0.0.0:{}", port);
    println!("{} Server running at {}", "✓".green(), format!("http://{}", addr).bright_blue());

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}