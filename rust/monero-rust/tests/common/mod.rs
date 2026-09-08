#![allow(dead_code)]

pub mod offline;

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::OnceLock;
use std::time::Duration;

pub const STAGENET_NODE: &str = "http://127.0.0.1:38081";
const STAGENET_ADDR: &str = "127.0.0.1:38081";

static STAGENET_AVAILABLE: OnceLock<bool> = OnceLock::new();

/// Check that the local node serves stagenet.
///
/// A listening daemon may belong to another test setup and serve a different
/// chain. Tests that depend on stagenet blocks must check `/get_info` first.
pub fn stagenet_available() -> bool {
    *STAGENET_AVAILABLE.get_or_init(|| match node_nettype() {
        Some(nettype) if nettype == "stagenet" => true,
        Some(nettype) => {
            eprintln!(
                "Skipping live tests: {} is serving a '{}' chain, not stagenet",
                STAGENET_NODE, nettype
            );
            false
        }
        None => {
            eprintln!(
                "Skipping live tests: no node reachable at {}",
                STAGENET_NODE
            );
            false
        }
    })
}

/// Find the regtest node configured for this test run.
///
/// `scripts/regtest-node.sh start` chooses an available port and prints the
/// `MONERO_REGTEST_RPC` assignment.
/// Skip if `MONERO_REGTEST_RPC` is unset. If it is set, require a working
/// regtest daemon so a startup failure cannot silently skip the tests.
pub fn regtest_node() -> Option<String> {
    let Ok(url) = std::env::var("MONERO_REGTEST_RPC") else {
        eprintln!("Skipping regtest tests: MONERO_REGTEST_RPC is unset");
        return None;
    };

    let authority = url
        .trim_end_matches('/')
        .rsplit("//")
        .next()
        .unwrap_or_default()
        .to_string();

    match node_nettype_of(&authority) {
        Some(nettype) if nettype == "fakechain" => Some(url),
        Some(nettype) => panic!(
            "MONERO_REGTEST_RPC points at {}, which is serving a '{}' chain, not regtest",
            url, nettype
        ),
        None => panic!(
            "MONERO_REGTEST_RPC is set to {} but nothing there answers /get_info - \
             a half-started daemon looks alive to `regtest-node.sh status` but cannot serve",
            url
        ),
    }
}

/// Read the local daemon's network from `/get_info`.
///
/// Use `TcpStream` so this check needs neither an async runtime nor the RPC
/// client under test.
fn node_nettype() -> Option<String> {
    node_nettype_of(STAGENET_ADDR)
}

fn node_nettype_of(authority: &str) -> Option<String> {
    let addr: SocketAddr = authority.parse().ok()?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(1)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .ok()?;

    stream
        .write_all(
            b"GET /get_info HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nAccept: */*\r\n\r\n",
        )
        .ok()?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response).ok()?;
    json_string_field(&String::from_utf8_lossy(&response), "nettype")
}

/// Extract a string field from the `/get_info` JSON response.
///
/// `tests/live_gate_test.rs` checks this parser because a parsing failure
/// would otherwise silently skip the stagenet tests.
pub fn json_string_field(haystack: &str, field: &str) -> Option<String> {
    let key = format!("\"{}\"", field);
    let after_key = &haystack[haystack.find(&key)? + key.len()..];
    let after_colon = &after_key[after_key.find(':')? + 1..];
    let start = after_colon.find('"')? + 1;
    let value = &after_colon[start..];
    Some(value[..value.find('"')?].to_string())
}
