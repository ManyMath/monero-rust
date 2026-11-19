#![cfg(not(target_arch = "wasm32"))]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use monero_rust::monero_rpc::{
    HttpRpc, HttpTimeouts, Rpc, RpcConnection, RpcError, MAX_ERROR_TEXT, MAX_REQUEST_BYTES,
};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

type Reply = Arc<dyn Fn(&str, &[u8]) -> Result<Vec<u8>, RpcError> + Send + Sync>;

#[derive(Clone)]
struct Node {
    reply: Reply,
    requests: Arc<AtomicUsize>,
}

impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Node")
    }
}

#[async_trait]
impl RpcConnection for Node {
    async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
        self.requests.fetch_add(1, Ordering::SeqCst);
        (self.reply)(route, &body)
    }
}

fn node(
    reply: impl Fn(&str, &[u8]) -> Result<Vec<u8>, RpcError> + Send + Sync + 'static,
) -> (Rpc<Node>, Arc<AtomicUsize>) {
    let requests = Arc::new(AtomicUsize::new(0));
    let node = Node {
        reply: Arc::new(reply),
        requests: requests.clone(),
    };
    (Rpc::new_with_connection(node), requests)
}

fn json_node(reply: Value) -> Rpc<Node> {
    node(move |_, _| Ok(serde_json::to_vec(&reply).unwrap())).0
}

fn binary_node(reply: Vec<u8>) -> Rpc<Node> {
    node(move |_, _| Ok(reply.clone())).0
}

#[derive(serde::Serialize)]
struct BinaryStatus {
    status: String,
    untrusted: bool,
}

fn binary_status(status: &str) -> Vec<u8> {
    monero_epee_bin_serde::to_bytes(&BinaryStatus {
        status: status.to_string(),
        untrusted: false,
    })
    .unwrap()
}

#[tokio::test]
async fn restricted_limits_are_reported_with_the_nodes_status() {
    let refusal = "Too many transactions requested in restricted mode";
    let rpc = json_node(json!({ "status": refusal, "untrusted": false }));
    assert_eq!(
        rpc.get_transactions(&[[1; 32]]).await.unwrap_err(),
        RpcError::NodeStatus(refusal.to_string())
    );

    let refusal = "Too many key images queried in restricted mode";
    let rpc = json_node(json!({ "status": refusal, "untrusted": false }));
    let error = monero_rust::scanner::key_image_spent_status(&rpc, &["00".repeat(32)])
        .await
        .unwrap_err();
    assert!(error.contains(refusal), "{error}");

    let refusal = "Too many outs requested";
    let rpc = binary_node(binary_status(refusal));
    assert_eq!(
        rpc.get_unlocked_outputs(&[1, 2], 100).await.unwrap_err(),
        RpcError::NodeStatus(refusal.to_string())
    );

    let rpc = binary_node(binary_status("BUSY"));
    assert_eq!(
        rpc.get_blocks_fast(&[[0; 32]], 0, true).await.unwrap_err(),
        RpcError::NodeStatus("BUSY".to_string())
    );
    assert_eq!(
        rpc.get_o_indexes([0; 32]).await.unwrap_err(),
        RpcError::NodeStatus("BUSY".to_string())
    );
    assert_eq!(
        rpc.get_output_distribution(0, 0).await.unwrap_err(),
        RpcError::NodeStatus("BUSY".to_string())
    );
}

#[tokio::test]
async fn a_complete_reply_with_a_status_other_than_ok_is_refused() {
    let rpc = json_node(json!({ "status": "Failed", "txs": [] }));
    assert_eq!(
        rpc.get_transactions(&[[1; 32]]).await.unwrap_err(),
        RpcError::NodeStatus("Failed".to_string())
    );

    #[derive(serde::Serialize)]
    struct Indexes {
        credits: u64,
        o_indexes: Vec<u64>,
        status: String,
        top_hash: String,
        untrusted: bool,
    }
    let rpc = binary_node(
        monero_epee_bin_serde::to_bytes(&Indexes {
            credits: 0,
            o_indexes: vec![1, 2],
            status: "PAYMENT REQUIRED".to_string(),
            top_hash: String::new(),
            untrusted: false,
        })
        .unwrap(),
    );
    assert_eq!(
        rpc.get_o_indexes([0; 32]).await.unwrap_err(),
        RpcError::NodeStatus("PAYMENT REQUIRED".to_string())
    );
}

#[tokio::test]
async fn json_rpc_refusals_are_reported() {
    let rpc = json_node(json!({
        "error": { "code": -19, "message": "Too many block headers requested." },
        "id": "0",
        "jsonrpc": "2.0",
    }));
    assert_eq!(
        rpc.get_block_hash(1).await.unwrap_err(),
        RpcError::JsonRpcError {
            code: -19,
            message: "Too many block headers requested.".to_string()
        }
    );

    let rpc = json_node(json!({ "id": "0", "jsonrpc": "2.0", "result": { "status": "BUSY" } }));
    assert_eq!(
        rpc.get_block_hash(1).await.unwrap_err(),
        RpcError::NodeStatus("BUSY".to_string())
    );
}

#[tokio::test]
async fn an_unreadable_reply_without_a_refusal_is_a_parse_error() {
    let rpc = json_node(json!({ "status": "OK" }));
    assert_eq!(
        rpc.get_transactions(&[[1; 32]]).await.unwrap_err(),
        RpcError::InternalError("Failed to parse JSON response")
    );
    let rpc = binary_node(binary_status("OK"));
    assert_eq!(
        rpc.get_o_indexes([0; 32]).await.unwrap_err(),
        RpcError::InternalError("Failed to parse binary response")
    );
    let rpc = binary_node(b"not epee".to_vec());
    assert_eq!(
        rpc.get_o_indexes([0; 32]).await.unwrap_err(),
        RpcError::InternalError("Failed to parse binary response")
    );
}

#[tokio::test]
async fn text_from_the_node_is_kept_short_and_printable() {
    let long = format!("\u{1b}[31mBUSY\r\n{}", "x".repeat(64 << 10));
    let rpc = json_node(json!({ "status": long }));
    let RpcError::NodeStatus(status) = rpc.get_transactions(&[[1; 32]]).await.unwrap_err() else {
        panic!("expected a status");
    };
    assert!(
        status.chars().count() <= MAX_ERROR_TEXT,
        "{} chars",
        status.len()
    );
    assert!(!status.chars().any(char::is_control), "{status:?}");
    assert!(status.starts_with("[31mBUSY"), "{status:?}");

    let rpc = json_node(json!({ "error": { "code": -1, "message": long } }));
    let RpcError::JsonRpcError { message, .. } = rpc.get_block_hash(1).await.unwrap_err() else {
        panic!("expected a JSON-RPC error");
    };
    assert!(message.chars().count() <= MAX_ERROR_TEXT);
    assert!(!message.chars().any(char::is_control));

    let RpcError::HttpStatus { body, .. } = RpcError::http_status(500, long.as_bytes()) else {
        unreachable!()
    };
    assert!(body.chars().count() <= MAX_ERROR_TEXT);
    let RpcError::HttpStatus { body, .. } = RpcError::http_status(500, &[0xff; 4096]) else {
        unreachable!()
    };
    assert!(body.chars().count() <= MAX_ERROR_TEXT);
}

#[tokio::test]
async fn a_request_larger_than_monerod_reads_is_not_sent() {
    let (rpc, requests) = node(|_, _| Ok(br#"{"status":"OK","spent_status":[]}"#.to_vec()));
    let key_images: Vec<String> = (0..16_000).map(|_| "00".repeat(32)).collect();
    let error = rpc
        .rpc_call::<_, Value>(
            "is_key_image_spent",
            Some(json!({ "key_images": key_images })),
        )
        .await
        .unwrap_err();
    let RpcError::RequestTooLarge { size, limit } = error else {
        panic!("expected RequestTooLarge, got {error:?}");
    };
    assert_eq!(limit, MAX_REQUEST_BYTES);
    assert!(size > MAX_REQUEST_BYTES);
    assert!(MAX_REQUEST_BYTES < 1_048_576);
    assert_eq!(requests.load(Ordering::SeqCst), 0, "nothing was sent");

    let (rpc, requests) = node(|_, body| {
        let request: Value = serde_json::from_slice(body).unwrap();
        let count = request["key_images"].as_array().unwrap().len();
        Ok(serde_json::to_vec(&json!({ "status": "OK", "spent_status": vec![0; count] })).unwrap())
    });
    assert_eq!(
        monero_rust::scanner::key_image_spent_status(&rpc, &key_images)
            .await
            .unwrap()
            .len(),
        16_000
    );
    assert_eq!(requests.load(Ordering::SeqCst), 16);
}

async fn http_node(head: String, body: Vec<u8>) -> (String, Arc<AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let accepted = Arc::new(AtomicUsize::new(0));
    let counter = accepted.clone();
    let body = Arc::new(body);
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            counter.fetch_add(1, Ordering::SeqCst);
            let head = head.clone();
            let body = body.clone();
            tokio::spawn(async move {
                let mut seen = Vec::new();
                let mut buffer = [0u8; 4096];
                while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut buffer).await {
                        Ok(0) | Err(_) => return,
                        Ok(read) => seen.extend_from_slice(&buffer[..read]),
                    }
                }
                if stream.write_all(head.as_bytes()).await.is_ok() {
                    let _ = stream.write_all(&body).await;
                }
            });
        }
    });
    (url, accepted)
}

fn client(url: String) -> Rpc<HttpRpc> {
    HttpRpc::with_timeouts(
        url,
        HttpTimeouts {
            connect: Duration::from_secs(5),
            idle: Duration::from_secs(5),
            total: Duration::from_secs(10),
            min_bytes_per_sec: u64::MAX,
        },
    )
    .unwrap()
}

#[tokio::test]
async fn a_route_restricted_rpc_does_not_serve_is_reported_as_http_404() {
    let head = "HTTP/1.1 404 Not found\r\nServer: Epee-based\r\nContent-Length: 0\r\n\
                Content-Type: text/plain\r\n\r\n"
        .to_string();
    let (url, accepted) = http_node(head, vec![]).await;
    let rpc = client(url);
    let error = rpc
        .rpc_call::<Value, Value>("get_transaction_pool", None)
        .await
        .unwrap_err();
    assert_eq!(
        error,
        RpcError::HttpStatus {
            status: 404,
            body: String::new()
        }
    );
    assert_eq!(error.to_string(), "node answered HTTP 404");
    assert_eq!(accepted.load(Ordering::SeqCst), 1, "not retried");
}

#[tokio::test]
async fn a_failed_request_is_reported_as_http_500_with_a_short_excerpt() {
    let (url, accepted) = http_node(
        "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n".to_string(),
        vec![],
    )
    .await;
    let rpc = client(url);
    assert_eq!(
        rpc.get_blocks_fast(&[[0; 32]], 1_380_000, false)
            .await
            .unwrap_err(),
        RpcError::HttpStatus {
            status: 500,
            body: String::new()
        }
    );
    assert_eq!(accepted.load(Ordering::SeqCst), 1, "not retried");

    const BODY: usize = 64 << 20;
    let mut body = b"node failed\n".to_vec();
    body.resize(BODY, b'A');
    let (url, accepted) = http_node(
        format!("HTTP/1.1 500 Internal Server Error\r\nContent-Length: {BODY}\r\n\r\n"),
        body,
    )
    .await;
    let rpc = client(url);
    let error = rpc.get_height().await.unwrap_err();
    let RpcError::HttpStatus { status, body } = &error else {
        panic!("expected an HTTP status, got {error:?}");
    };
    assert_eq!(*status, 500);
    assert!(body.starts_with("node failed AAA"), "{body:?}");
    assert!(body.chars().count() <= MAX_ERROR_TEXT);
    assert!(error.to_string().len() < 2 * MAX_ERROR_TEXT);
    assert_eq!(accepted.load(Ordering::SeqCst), 1, "not retried");
}

#[derive(serde::Deserialize)]
struct PrunedTxFixture {
    as_hex_full: String,
    pruned_as_hex: String,
    prunable_hash: String,
    txid: String,
}

fn pruned_fixture() -> PrunedTxFixture {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/vectors/pruned_tx_fixture_0.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn entry_node(entry: Value) -> Rpc<Node> {
    json_node(json!({ "status": "OK", "untrusted": false, "txs": [entry] }))
}

#[tokio::test]
async fn a_pruned_transaction_is_reported_as_pruned() {
    let fixture = pruned_fixture();
    let txid: [u8; 32] = hex::decode(&fixture.txid).unwrap().try_into().unwrap();
    let pruned_entry = |prunable_hash: &str, pruned_as_hex: &str| {
        entry_node(json!({
            "as_hex": "",
            "prunable_as_hex": "",
            "prunable_hash": prunable_hash,
            "pruned_as_hex": pruned_as_hex,
            "tx_hash": fixture.txid,
        }))
    };

    let rpc = pruned_entry(&fixture.prunable_hash, &fixture.pruned_as_hex);
    let error = rpc.get_transaction(txid).await.unwrap_err();
    assert_eq!(error, RpcError::PrunedTransaction(txid));
    let message = error.to_string();
    assert!(message.contains(&fixture.txid), "{message}");
    assert!(message.contains("unpruned node"), "{message}");

    let rpc = pruned_entry("", &fixture.pruned_as_hex);
    assert_eq!(
        rpc.get_transaction(txid).await.unwrap_err(),
        RpcError::PrunedTransaction(txid)
    );

    let rpc = pruned_entry(&"11".repeat(32), &fixture.pruned_as_hex);
    assert_eq!(
        rpc.get_transaction(txid).await.unwrap_err(),
        RpcError::InvalidNode
    );

    let rpc = pruned_entry(&fixture.prunable_hash, "0011");
    assert_eq!(
        rpc.get_transaction(txid).await.unwrap_err(),
        RpcError::InvalidTransaction(txid)
    );

    let rpc = entry_node(json!({
        "as_hex": fixture.as_hex_full,
        "prunable_as_hex": "",
        "prunable_hash": fixture.prunable_hash,
        "pruned_as_hex": "",
        "tx_hash": fixture.txid,
    }));
    assert_eq!(rpc.get_transaction(txid).await.unwrap().hash(), txid);
}
