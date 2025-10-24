use std::time::Duration;

use forge::crdt::{Operation, OperationType, Position};
use tokio::time::sleep;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ws_roundtrip() {
    let port: u16 = 43111;
    let path = std::path::PathBuf::from(".");

    // Start server
    let server = tokio::spawn(async move {
        let _ = forge::server::start(port, path).await;
    });

    sleep(Duration::from_millis(200)).await;

    // Connect WS
    let url = format!("ws://127.0.0.1:{}/ws", port);
    let (ws, _) = tokio_tungstenite::connect_async(url).await.expect("ws connect");
    let (mut write, mut read) = ws.split();

    // Send an operation
    let op = Operation::new(
        "tests/tmp.txt".to_string(),
        OperationType::Insert {
            position: Position::new(1, 1, 0, "clientA".to_string(), 1),
            content: "x".to_string(),
            length: 1,
        },
        "clientA".to_string(),
    );
    let op_id = op.id;
    let text = serde_json::to_string(&op).unwrap();
    use futures::SinkExt;
    write.send(tokio_tungstenite::tungstenite::Message::Text(text.into()))
        .await
        .unwrap();

    // Receive at least one broadcast
    use futures::StreamExt;
    let mut got_back = false;
    let start = std::time::Instant::now();
    while let Some(msg) = read.next().await {
        if start.elapsed() > Duration::from_secs(3) { break; }
        if let Ok(tokio_tungstenite::tungstenite::Message::Text(t)) = msg {
            let s = t.to_string();
            if let Ok(o) = serde_json::from_str::<Operation>(&s) {
                if o.id == op_id { got_back = true; break; }
            }
        }
    }

    assert!(got_back, "did not get our operation broadcast back");

    server.abort();
}
