use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use forge::crdt::OperationType;
use forge::storage::{self, Database, OperationLog};
use forge::sync::{SyncManager, remote::connect_peer};
use tempfile::TempDir;
use tokio::time::{sleep, timeout};

fn reserve_port() -> std::io::Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn init_watch_sync_workflow() -> Result<()> {
    let original_dir = std::env::current_dir()?;
    let temp_dir = TempDir::new()?;
    let repo_path = temp_dir.path().to_path_buf();

    storage::init(repo_path.as_path()).await?;

    let port = reserve_port()?;

    let server_handle = tokio::spawn({
        let repo = repo_path.clone();
        async move {
            let _ = forge::server::start(port, repo).await;
        }
    });

    sleep(Duration::from_millis(150)).await;

    let watch_handle = tokio::spawn({
        let repo = repo_path.clone();
        async move {
            let _ = forge::watcher::watch(repo, true, vec![format!("ws://127.0.0.1:{}/ws", port)])
                .await;
        }
    });

    sleep(Duration::from_millis(250)).await;

    let client_store = repo_path.join(".dx_client/forge");
    tokio::fs::create_dir_all(&client_store).await?;
    let client_db = Arc::new(Database::new(&client_store)?);
    client_db.initialize()?;
    let client_oplog = Arc::new(OperationLog::new(client_db.clone()));
    let client_sync = SyncManager::new();
    let client_handle = connect_peer(
        &format!("ws://127.0.0.1:{}/ws", port),
        "test-client".into(),
        "test-repo".into(),
        client_sync.clone(),
        client_oplog.clone(),
    )
    .await?;

    let mut client_rx = client_sync.subscribe();

    let tracked_file = repo_path.join("hello.txt");
    tokio::fs::write(&tracked_file, "hello world").await?;

    let received = timeout(Duration::from_secs(5), async {
        loop {
            match client_rx.recv().await {
                Ok(op) => {
                    if op.file_path.ends_with("hello.txt") {
                        break op;
                    }
                }
                Err(err) => {
                    println!("client sync channel error: {err}");
                }
            }
        }
    })
    .await?;

    if !matches!(&received.op_type, OperationType::FileCreate { .. }) {
        return Err(anyhow!("expected file create operation"));
    }

    std::env::set_current_dir(&repo_path)?;
    storage::time_travel(&tracked_file, None).await?;
    std::env::set_current_dir(&original_dir)?;

    client_handle.abort();
    watch_handle.abort();
    server_handle.abort();

    Ok(())
}
