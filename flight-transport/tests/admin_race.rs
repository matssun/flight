// SPDX-License-Identifier: MIT

//! The operator socket's client against a server that answers the moment it has read the request
//! line and closes: the client must still get the answer. The client used to half-close its end
//! after writing, which macOS refuses (`ENOTCONN`) when the server has already gone; that failed
//! `flight orchestrator enrollment create` now and then in CI.

use flight_transport::admin_request;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_answer_that_arrives_before_the_client_half_closes_is_still_an_answer() {
    let dir = std::env::temp_dir().join(format!("flight-admin-race-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("admin.sock");
    let listener = UnixListener::bind(&path).expect("bind");
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                let _ = reader.read_line(&mut line).await;
                let mut stream = reader.into_inner();
                let _ = stream.write_all(b"ok answered").await;
                // Dropped at once: no waiting for the client to finish.
            });
        }
    });
    for i in 0..20_000 {
        let reply = admin_request(&path, "status")
            .await
            .unwrap_or_else(|e| panic!("request {i} failed: {e}"));
        assert_eq!(reply, "answered");
    }
}
