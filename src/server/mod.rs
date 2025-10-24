pub mod api;

use anyhow::Result;
use std::path::PathBuf;

pub async fn start(port: u16, path: PathBuf) -> Result<()> {
    api::serve(port, path).await
}
