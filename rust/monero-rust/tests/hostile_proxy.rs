//! A mutating HTTP proxy in front of a live daemon, to show wallet2 the
//! crate's hostile answers. Run by hand:
//!
//!     MONERO_REGTEST_RPC=http://127.0.0.1:PORT \
//!     HOSTILE_PROXY_MUTATION=distribution-truncated \
//!     HOSTILE_PROXY_DIR=/path/to/scratch \
//!     cargo test -p monero-rust --test hostile_proxy -- --ignored --nocapture
//!
//! It exits when a file named `stop` appears in that directory.

#![cfg(not(target_arch = "wasm32"))]

mod hostile;

use std::io::Write;
use std::path::PathBuf;

use hostile::epee::{self, Obj, Val};
use hostile::LiveNode;
use monero_rust::monero_rpc::RpcConnection;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn block_701(reply: &mut Obj, edit: impl FnOnce(&mut Obj)) {
    let start = epee::get(reply, "start_height").map_or(0, Val::u64);
    let Some(blocks) = epee::get_mut(reply, "blocks") else {
        return;
    };
    let blocks = blocks.arr_mut();
    if start <= 701 && 701 - start < blocks.len() as u64 {
        edit(blocks[(701 - start) as usize].obj_mut());
    }
}

fn mutate(mutation: &str, route: &str, reply: Vec<u8>) -> Vec<u8> {
    let edit = |f: &dyn Fn(&mut Obj)| match epee::decode(&reply) {
        Ok(mut obj) => {
            f(&mut obj);
            epee::encode(&obj)
        }
        Err(_) => reply.clone(),
    };
    match (mutation, route) {
        ("distribution-truncated", "get_output_distribution.bin") => edit(&|obj| {
            let first = epee::get_mut(obj, "distributions").unwrap().arr_mut()[0].obj_mut();
            for field in ["compressed_data", "distribution"] {
                if let Some(Val::Str(data)) = epee::get_mut(first, field) {
                    data.truncate(100);
                }
            }
        }),
        ("block-version-byte", "getblocks.bin") => edit(&|obj| {
            block_701(obj, |block| {
                if let Some(Val::Str(blob)) = epee::get_mut(block, "block") {
                    blob[0] = 1;
                }
            })
        }),
        ("tx-blob-unparseable", "getblocks.bin") => edit(&|obj| {
            block_701(obj, |block| {
                let Some(txs) = epee::get_mut(block, "txs") else {
                    return;
                };
                match &mut txs.arr_mut()[0] {
                    Val::Str(blob) => *blob = vec![0xff; 64],
                    Val::Obj(tx) => epee::set(tx, "blob", Val::Str(vec![0xff; 64])),
                    _ => {}
                }
            })
        }),
        _ => reply,
    }
}

async fn serve(
    mut stream: tokio::net::TcpStream,
    upstream: LiveNode,
    mutation: String,
    log: PathBuf,
) {
    let mut buffer = Vec::new();
    loop {
        let header_end = loop {
            if let Some(position) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                break position + 4;
            }
            let mut chunk = [0u8; 65536];
            match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => return,
                Ok(read) => buffer.extend_from_slice(&chunk[..read]),
            }
        };
        let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
        let route = head
            .split_whitespace()
            .nth(1)
            .unwrap_or("/")
            .trim_start_matches('/')
            .to_string();
        let length: usize = head
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse().ok())?
            })
            .unwrap_or(0);
        while buffer.len() < header_end + length {
            let mut chunk = [0u8; 65536];
            match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => return,
                Ok(read) => buffer.extend_from_slice(&chunk[..read]),
            }
        }
        let body = buffer[header_end..header_end + length].to_vec();
        buffer.drain(..header_end + length);

        let detail = if route == "json_rpc" {
            serde_json::from_slice::<serde_json::Value>(&body)
                .ok()
                .map(|v| v["method"].to_string())
                .unwrap_or_default()
        } else if route == "get_outs.bin" {
            epee::decode(&body)
                .map(|obj| {
                    let indices: Vec<u64> = epee::items(&obj, "outputs")
                        .iter()
                        .map(|out| epee::get(out.obj(), "index").unwrap().u64())
                        .collect();
                    format!("{indices:?}")
                })
                .unwrap_or_default()
        } else if route.ends_with(".bin") {
            String::new()
        } else {
            String::from_utf8_lossy(&body).chars().take(300).collect()
        };
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
        {
            let _ = writeln!(file, "{route} {detail}");
        }

        let reply = match upstream.post(&route, body).await {
            Ok(reply) => mutate(&mutation, &route, reply),
            Err(_) => return,
        };
        let kind = if route.ends_with(".bin") {
            "application/octet-stream"
        } else {
            "application/json"
        };
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n\r\n",
            reply.len()
        );
        if stream.write_all(head.as_bytes()).await.is_err()
            || stream.write_all(&reply).await.is_err()
        {
            return;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "runs a proxy for wallet2 until HOSTILE_PROXY_DIR/stop exists"]
async fn proxy() {
    let upstream = LiveNode::new(&std::env::var("MONERO_REGTEST_RPC").expect("MONERO_REGTEST_RPC"));
    let mutation = std::env::var("HOSTILE_PROXY_MUTATION").unwrap_or_else(|_| "none".into());
    let dir = PathBuf::from(std::env::var("HOSTILE_PROXY_DIR").expect("HOSTILE_PROXY_DIR"));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    std::fs::write(
        dir.join("port"),
        listener.local_addr().unwrap().port().to_string(),
    )
    .unwrap();
    let stop = dir.join("stop");
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted.unwrap();
                tokio::spawn(serve(stream, upstream.clone(), mutation.clone(), dir.join("requests.log")));
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(250)) => {
                if stop.exists() {
                    return;
                }
            }
        }
    }
}
