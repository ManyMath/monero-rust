#![cfg(not(target_arch = "wasm32"))]

mod hostile;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hostile::epee::{self, Val};
use monero_rust::monero_rpc::{Rpc, RpcConnection, RpcError};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Tracking;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn grow(size: usize) {
    let live = LIVE.fetch_add(size, Ordering::Relaxed) + size;
    PEAK.fetch_max(live, Ordering::Relaxed);
}

unsafe impl GlobalAlloc for Tracking {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        grow(layout.size());
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        grow(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        grow(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Tracking = Tracking;

// The allocation counters are process-wide: run these tests one at a time.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn peak_growth<F: std::future::Future>(f: F) -> (F::Output, usize) {
    let before = LIVE.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);
    let output = f.await;
    (output, PEAK.load(Ordering::Relaxed).saturating_sub(before))
}

#[derive(Clone, Debug)]
struct Canned(Arc<Vec<u8>>);

#[async_trait::async_trait]
impl RpcConnection for Canned {
    async fn post(&self, _route: &str, _body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
        Ok(self.0.as_ref().clone())
    }
}

fn canned(bytes: Vec<u8>) -> Rpc<Canned> {
    Rpc::new_with_connection(Canned(Arc::new(bytes)))
}

async fn all_binary_calls_fail(rpc: &Rpc<Canned>, what: &str) {
    assert!(
        rpc.get_blocks_fast(&[[0; 32]], 1, false).await.is_err(),
        "{what}: getblocks"
    );
    assert!(
        rpc.get_unlocked_outputs(&[1, 2], 10).await.is_err(),
        "{what}: get_outs"
    );
    assert!(
        rpc.get_output_distribution(0, 10).await.is_err(),
        "{what}: distribution"
    );
    assert!(
        rpc.get_o_indexes([0; 32]).await.is_err(),
        "{what}: get_o_indexes"
    );
}

fn with_header(body: &[u8]) -> Vec<u8> {
    let mut bytes = epee::HEADER.to_vec();
    bytes.extend_from_slice(body);
    bytes
}

#[tokio::test]
async fn epee_length_bombs_and_deep_nesting_are_refused_without_allocating() {
    let _serial = SERIAL.lock().await;
    let huge = (u64::MAX >> 2).to_le_bytes();
    let mut huge_varint = huge;
    huge_varint[0] |= 3;

    let mut cases: Vec<(&str, Vec<u8>)> = Vec::new();
    let mut body = vec![4, 6];
    body.extend_from_slice(b"blocks");
    body.push(0x8c);
    body.extend_from_slice(&huge_varint);
    cases.push(("array count larger than the body", with_header(&body)));
    let mut body = vec![4, 6];
    body.extend_from_slice(b"status");
    body.push(10);
    body.extend_from_slice(&huge_varint);
    cases.push(("string length larger than the body", with_header(&body)));
    cases.push((
        "field count larger than the body",
        with_header(&huge_varint),
    ));
    let mut body = Vec::new();
    for _ in 0..200_000 {
        body.extend_from_slice(&[4, 1, b'x', 12]);
    }
    body.push(0);
    cases.push(("200,000 nested objects", with_header(&body)));
    let mut body = Vec::new();
    for _ in 0..200_000 {
        body.extend_from_slice(&[4, 1, b'x', 0x8c, 4]);
    }
    body.push(0);
    cases.push(("200,000 nested object arrays", with_header(&body)));
    cases.push(("header only", epee::HEADER.to_vec()));
    cases.push(("empty", Vec::new()));
    cases.push(("trailing bytes", {
        let mut bytes = epee::encode(&vec![("status".into(), epee::str_val("OK"))]);
        bytes.push(0);
        bytes
    }));

    for (what, bytes) in cases {
        let size = bytes.len();
        let rpc = canned(bytes);
        let (_, growth) = peak_growth(all_binary_calls_fail(&rpc, what)).await;
        assert!(
            growth < 4 * size + (1 << 20),
            "{what}: {growth} bytes allocated while refusing a {size} byte reply"
        );
    }
}

#[tokio::test]
#[ignore = "known defect: a reply padded with empty array elements costs many times its size in memory"]
async fn padded_arrays_cost_at_most_eight_times_the_reply_size() {
    let _serial = SERIAL.lock().await;
    let count = 4usize << 20;
    let base = || -> epee::Obj {
        vec![
            ("status".into(), epee::str_val("OK")),
            ("start_height".into(), Val::U64(1)),
            ("current_height".into(), Val::U64(2)),
        ]
    };
    let block = hex::decode(&hostile::snapshot().chain[1].entry).unwrap();
    let block = epee::get(&epee::decode(&block).unwrap(), "block")
        .unwrap()
        .clone();

    let mut top_level = base();
    top_level.push((
        "output_indices".into(),
        Val::Arr(12, vec![Val::Obj(vec![]); count]),
    ));
    let mut nested = base();
    nested.push((
        "output_indices".into(),
        Val::Arr(
            12,
            vec![Val::Obj(vec![(
                "indices".into(),
                Val::Arr(12, vec![Val::Obj(vec![]); count]),
            )])],
        ),
    ));
    let blobs = |blob: Vec<u8>, count: usize| {
        let mut reply = base();
        reply.push((
            "blocks".into(),
            Val::Arr(
                12,
                vec![Val::Obj(vec![
                    ("block".into(), block.clone()),
                    ("txs".into(), Val::Arr(10, vec![Val::Str(blob); count])),
                ])],
            ),
        ));
        reply
    };
    let mut one_field = base();
    one_field.push((
        "output_indices".into(),
        Val::Arr(
            12,
            vec![Val::Obj(vec![("z".into(), Val::U8(0))]); count / 2],
        ),
    ));

    for (what, reply) in [
        ("empty index entries per block", top_level),
        ("empty index entries per transaction", nested),
        ("empty transaction blobs", blobs(vec![], count)),
        ("one-byte transaction blobs", blobs(vec![7], count)),
        ("31-byte transaction blobs", blobs(vec![7; 31], count / 16)),
        ("index entries with one unknown field", one_field),
    ] {
        let reply = epee::encode(&reply);
        let size = reply.len();
        let rpc = canned(reply);
        let (_, growth) = peak_growth(rpc.get_blocks_fast(&[[0; 32]], 1, false)).await;
        assert!(
            growth <= 8 * size,
            "{what}: a {size} byte getblocks reply cost {growth} bytes of heap ({}x)",
            growth / size
        );
    }
}

#[tokio::test]
async fn deeply_nested_json_is_refused() {
    let _serial = SERIAL.lock().await;
    let nested = "[".repeat(200_000);
    let rpc = canned(format!("{{\"height\":{nested}").into_bytes());
    assert!(rpc.get_height().await.is_err());
    let rpc = canned(format!("{{\"result\":{{\"block_header\":{nested}").into_bytes());
    assert!(rpc.get_rct_type().await.is_err());
}

async fn read_request(stream: &mut tokio::net::TcpStream) {
    let mut seen = Vec::new();
    let mut buffer = [0u8; 4096];
    while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(read) => seen.extend_from_slice(&buffer[..read]),
        }
    }
}

fn stall_budget() -> Duration {
    Duration::from_secs(
        std::env::var("HOSTILE_STALL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(20),
    )
}

fn short_timeouts() -> monero_rust::monero_rpc::HttpTimeouts {
    let defaults = monero_rust::monero_rpc::HttpTimeouts::default();
    assert_eq!(defaults.idle, Duration::from_secs(210));
    assert_eq!(defaults.total, Duration::from_secs(210));
    assert_eq!(defaults.connect, Duration::from_secs(30));
    monero_rust::monero_rpc::HttpTimeouts {
        connect: Duration::from_secs(2),
        idle: Duration::from_secs(2),
        total: Duration::from_secs(4),
        min_bytes_per_sec: u64::MAX,
    }
}

#[tokio::test]
async fn silent_node_is_ended_by_the_idle_timeout() {
    let _serial = SERIAL.lock().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let mut held = Vec::new();
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_request(&mut stream).await;
            held.push(stream);
        }
    });
    let rpc = monero_rust::monero_rpc::HttpRpc::with_timeouts(url, short_timeouts()).unwrap();
    let budget = stall_budget();
    let started = Instant::now();
    let outcome = tokio::time::timeout(budget, rpc.get_height()).await;
    assert!(
        matches!(outcome, Ok(Err(_))),
        "get_height was still waiting after {:?} on a node that never answers",
        started.elapsed()
    );
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "ended by the idle timeout"
    );
}

#[tokio::test]
async fn slow_drip_reply_is_ended_by_the_call_deadline() {
    let _serial = SERIAL.lock().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                read_request(&mut stream).await;
                let head = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                            Content-Length: 1000000\r\n\r\n";
                if stream.write_all(head.as_bytes()).await.is_err() {
                    return;
                }
                loop {
                    if stream.write_all(b" ").await.is_err() {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            });
        }
    });
    let rpc = monero_rust::monero_rpc::HttpRpc::with_timeouts(url, short_timeouts()).unwrap();
    let budget = stall_budget();
    let started = Instant::now();
    let outcome = tokio::time::timeout(budget, rpc.get_height()).await;
    assert!(
        matches!(outcome, Ok(Err(_))),
        "get_height was still reading after {:?} from a node sending 5 bytes a second",
        started.elapsed()
    );
    assert!(
        started.elapsed() >= Duration::from_secs(4),
        "ended by the call deadline"
    );
}

#[tokio::test]
async fn dropped_connections_are_retried_for_read_only_calls_only() {
    let _serial = SERIAL.lock().await;
    async fn node(failures: usize) -> (String, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let accepted = Arc::new(AtomicUsize::new(0));
        let counter = accepted.clone();
        tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                if counter.fetch_add(1, Ordering::SeqCst) < failures {
                    continue;
                }
                tokio::spawn(async move {
                    read_request(&mut stream).await;
                    let body = "{\"height\":7,\"status\":\"OK\"}";
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(reply.as_bytes()).await;
                });
            }
        });
        (url, accepted)
    }
    let client = |url: String| monero_rust::monero_rpc::HttpRpc::new(url).unwrap();

    let (url, accepted) = node(2).await;
    assert_eq!(client(url).get_height().await.unwrap(), 7);
    assert_eq!(accepted.load(Ordering::SeqCst), 3);

    let (url, accepted) = node(3).await;
    assert!(client(url).get_height().await.is_err());
    assert_eq!(
        accepted.load(Ordering::SeqCst),
        3,
        "two retries, then give up"
    );

    let (url, accepted) = node(1).await;
    let relay: Result<serde_json::Value, _> = client(url)
        .rpc_call(
            "send_raw_transaction",
            Some(serde_json::json!({ "tx_as_hex": "00", "do_not_relay": false })),
        )
        .await;
    assert!(relay.is_err());
    assert_eq!(accepted.load(Ordering::SeqCst), 1, "a relay is sent once");
}

#[tokio::test]
async fn reply_body_is_cut_off_at_the_size_limit() {
    let _serial = SERIAL.lock().await;
    const BODY: usize = 256 << 20;
    const ACCEPTABLE: usize = 32 << 20;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let sent = Arc::new(AtomicUsize::new(0));
    let counter = sent.clone();
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let counter = counter.clone();
            tokio::spawn(async move {
                read_request(&mut stream).await;
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {BODY}\r\n\r\n"
                );
                if stream.write_all(head.as_bytes()).await.is_err() {
                    return;
                }
                let chunk = vec![b' '; 1 << 20];
                for _ in 0..(BODY >> 20) {
                    if stream.write_all(&chunk).await.is_err() {
                        return;
                    }
                    counter.fetch_add(chunk.len(), Ordering::SeqCst);
                }
            });
        }
    });
    let result = monero_rust::get_daemon_height(&url).await;
    assert!(result.is_err(), "a body of spaces is not a height");
    tokio::time::sleep(Duration::from_millis(200)).await;
    let taken = sent.load(Ordering::SeqCst);
    assert!(
        taken <= ACCEPTABLE,
        "the wallet read {} MiB of reply to a get_height request before giving up",
        taken >> 20
    );
}
