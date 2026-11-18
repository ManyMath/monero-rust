//! What the crate sends to a node, compared byte for byte with what wallet2
//! sent (`tests/vectors/wallet2_requests.json`) apart from the `client` value.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use monero_rust::monero_rpc::{Rpc, RpcConnection, RpcError};
use serde_json::Value;

fn wallet2(name: &str) -> (String, Vec<u8>) {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/vectors/wallet2_requests.json");
    let vectors: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let request = &vectors["requests"][name];
    let route = request["request_line"]
        .as_str()
        .unwrap()
        .split(' ')
        .nth(1)
        .unwrap()
        .trim_start_matches('/')
        .to_string();
    (
        route,
        hex::decode(request["body_hex"].as_str().unwrap()).unwrap(),
    )
}

fn client_in(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    let bytes = text.as_bytes();
    let mut start = 0;
    while start + 208 <= bytes.len() {
        let run = bytes[start..]
            .iter()
            .take_while(|byte| byte.is_ascii_hexdigit())
            .count();
        if run == 208
            && (start == 0 || bytes[start - 1] == b'"' || bytes[start - 1] == 0x03)
            && bytes
                .get(start + 208)
                .map_or(true, |byte| !byte.is_ascii_hexdigit())
        {
            return text[start..start + 208].to_string();
        }
        start += run.max(1);
    }
    panic!("no client member in {text}");
}

fn with_client(wallet2: &[u8], ours: &str) -> Vec<u8> {
    let theirs = client_in(wallet2);
    let text = wallet2.to_vec();
    let at = text
        .windows(208)
        .position(|window| window == theirs.as_bytes())
        .unwrap();
    let mut out = text[..at].to_vec();
    out.extend_from_slice(ours.as_bytes());
    out.extend_from_slice(&text[at + 208..]);
    out
}

fn assert_same_request(ours: &(String, Vec<u8>), wallet2: &(String, Vec<u8>)) {
    assert_eq!(ours.0, wallet2.0, "route");
    let client = client_in(&ours.1);
    let micros = u64::from_str_radix(&client[64..80], 16).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros() as u64;
    assert!(
        now.abs_diff(micros) < 600_000_000,
        "timestamp is now, in microseconds"
    );
    let expected = with_client(&wallet2.1, &client);
    assert_eq!(
        String::from_utf8_lossy(&ours.1),
        String::from_utf8_lossy(&expected),
        "body of {}",
        ours.0
    );
}

#[derive(Clone, Debug, Default)]
struct Recorder {
    requests: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
    replies: Arc<Vec<(&'static str, Vec<u8>)>>,
}

impl Recorder {
    fn new(replies: Vec<(&'static str, Vec<u8>)>) -> Self {
        Recorder {
            requests: Arc::default(),
            replies: Arc::new(replies),
        }
    }

    fn rpc(&self) -> Rpc<Recorder> {
        Rpc::new_with_connection(self.clone())
    }

    fn requests(&self) -> Vec<(String, Vec<u8>)> {
        self.requests.lock().unwrap().clone()
    }
}

#[async_trait]
impl RpcConnection for Recorder {
    async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
        self.requests
            .lock()
            .unwrap()
            .push((route.to_string(), body.clone()));
        let method = serde_json::from_slice::<Value>(&body)
            .ok()
            .and_then(|request| request["method"].as_str().map(str::to_string));
        self.replies
            .iter()
            .find(|(key, _)| *key == route || Some(key.to_string()) == method)
            .map(|(_, reply)| reply.clone())
            .ok_or(RpcError::ConnectionError)
    }
}

const RELAY_OK: &str = r#"{"double_spend": false, "fee_too_low": false, "invalid_input": false,
    "invalid_output": false, "low_mixin": false, "not_relayed": false, "overspend": false,
    "reason": "", "status": "OK", "too_big": false, "too_few_outputs": false}"#;

#[tokio::test]
async fn relay_is_sent_to_wallet2s_route_with_wallet2s_body() {
    let wallet2_relay = wallet2("sendrawtransaction");
    let request: Value = serde_json::from_slice(&wallet2_relay.1).unwrap();
    let tx_hex = request["tx_as_hex"].as_str().unwrap();

    let node = Recorder::new(vec![("sendrawtransaction", RELAY_OK.as_bytes().to_vec())]);
    let mut state = monero_rust::WalletState::new();
    monero_rust::native::broadcast_raw_transaction_with_rpc(&node.rpc(), tx_hex, &mut state, false)
        .await
        .unwrap();
    let tx =
        monero_oxide::transaction::Transaction::read(&mut hex::decode(tx_hex).unwrap().as_slice())
            .unwrap();
    node.rpc().publish_transaction(&tx).await.unwrap();

    let requests = node.requests();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert_same_request(request, &wallet2_relay);
    }
}
