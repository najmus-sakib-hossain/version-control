use forge::crdt::{Operation, OperationType, Position};
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ws_cross_process_broadcast() {
    let port: u16 = 43112;
    let path = std::path::PathBuf::from(".");

    // Start server
    let server = tokio::spawn(async move {
        let _ = forge::server::start(port, path).await;
    });

    sleep(Duration::from_millis(200)).await;

    // Two clients
    let url = format!("ws://127.0.0.1:{}/ws", port);
    let (ws_a, _) = tokio_tungstenite::connect_async(url.clone())
        .await
        .expect("ws A");
    let (ws_b, _) = tokio_tungstenite::connect_async(url.clone())
        .await
        .expect("ws B");

    let (mut write_a, mut read_a) = ws_a.split();
    let (_write_b, mut read_b) = ws_b.split();

    // Client A sends op
    let op = Operation::new(
        "tests/tmp.txt".to_string(),
        OperationType::Insert {
            position: Position::new(1, 1, 0, "clientA".to_string(), 1),
            content: "y".to_string(),
            length: 1,
        },
        "clientA".to_string(),
    );
    let op_id = op.id;
    let text = serde_json::to_string(&op).unwrap();
    use futures::SinkExt;
    write_a
        .send(tokio_tungstenite::tungstenite::Message::Text(text.into()))
        .await
        .unwrap();

    // Client B should receive it
    use futures::StreamExt;
    let mut got_from_b = false;
    let start = std::time::Instant::now();
    while let Some(msg) = read_b.next().await {
        if start.elapsed() > Duration::from_secs(3) {
            break;
        }
        if let Ok(tokio_tungstenite::tungstenite::Message::Text(t)) = msg {
            let s = t.to_string();
            if let Ok(o) = serde_json::from_str::<Operation>(&s) {
                if o.id == op_id {
                    got_from_b = true;
                    break;
                }
            }
        }
    }

    assert!(got_from_b, "client B did not receive op from A");

    // Drain any messages from A to keep clean
    let _ = read_a.next().await;

    server.abort();
}
