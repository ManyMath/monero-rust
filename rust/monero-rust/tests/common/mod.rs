#![allow(dead_code)]

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
            eprintln!("Skipping live tests: no node reachable at {}", STAGENET_NODE);
            false
        }
    })
}

/// A private regtest node to run live tests against, if one was started for us.
///
/// `scripts/regtest-node.sh start` takes a free port and exports it as
/// `MONERO_REGTEST_RPC`. Nothing is hardcoded, so this can't collide with whatever else is
/// on the machine - which is the failure the stagenet port kept producing.
pub fn regtest_node() -> Option<String> {
    let url = std::env::var("MONERO_REGTEST_RPC").ok()?;
    let authority = url.trim_end_matches('/').rsplit("//").next()?.to_string();

    match node_nettype_of(&authority) {
        Some(nettype) if nettype == "fakechain" => Some(url),
        Some(nettype) => {
            eprintln!("Skipping regtest tests: {} is a '{}' chain, not regtest", url, nettype);
            None
        }
        None => {
            eprintln!("Skipping regtest tests: nothing reachable at {}", url);
            None
        }
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
    stream.set_write_timeout(Some(Duration::from_secs(2))).ok()?;

    stream
        .write_all(
            b"GET /get_info HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nAccept: */*\r\n\r\n",
        )
        .ok()?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response).ok()?;
    json_string_field(&String::from_utf8_lossy(&response), "nettype")
}

/// Pull `"<field>": "<value>"` out of a JSON blob, so the gate needn't take a parser
/// dependency of its own.
///
/// The gate fails closed, so if this ever stopped working it would disable every
/// live test while the suite stayed green. `tests/live_gate_test.rs` pins it.
pub fn json_string_field(haystack: &str, field: &str) -> Option<String> {
    let key = format!("\"{}\"", field);
    let after_key = &haystack[haystack.find(&key)? + key.len() ..];
    let after_colon = &after_key[after_key.find(':')? + 1 ..];
    let start = after_colon.find('"')? + 1;
    let value = &after_colon[start ..];
    Some(value[.. value.find('"')?].to_string())
}
