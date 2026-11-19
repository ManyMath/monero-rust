//! Monero daemon RPC client, ported from the vendored serai mirror onto the
//! monero-oxide types.
//!
//! monero-oxide's `interface` crate is trait-only: it ships no HTTP client,
//! so this module supplies a generic connection interface and native HTTP transport.
//! It supports JSON-RPC, binary Epee endpoints, `/getblocks.bin` output indices,
//! pruned transactions, and digest authentication.

#![allow(dead_code)]

use std::fmt::Debug;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

use async_trait::async_trait;
use thiserror::Error;

use curve25519_dalek::edwards::{CompressedEdwardsY, EdwardsPoint};

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};

#[cfg(not(target_arch = "wasm32"))]
use digest_auth::AuthContext;
#[cfg(not(target_arch = "wasm32"))]
use reqwest::Client;

use monero_oxide::{
    block::Block,
    ringct::RctType,
    transaction::{Input, NotPruned, Timelock, Transaction},
};
use monero_wallet::interface::FeeRate;

pub use crate::rpc_client_id::RpcClientKey;
use crate::wallet_state::current_time_secs;

/// A node's fee estimate: piconero per byte of weight, and the quantization mask.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fee {
    pub per_weight: u64,
    pub mask: u64,
}

/// wallet2's four daemon fee tiers (2021 fee scaling) and its automatic default.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FeePriority {
    /// wallet2's automatic priority: tier one unless the pool or recent blocks
    /// are congested, then tier two.
    #[default]
    Automatic,
    Low,
    Normal,
    Elevated,
    Priority,
}

impl FeePriority {
    fn tier(self) -> Option<usize> {
        match self {
            Self::Automatic => None,
            Self::Low => Some(0),
            Self::Normal => Some(1),
            Self::Elevated => Some(2),
            Self::Priority => Some(3),
        }
    }
}

impl Fee {
    /// The estimate as a `FeeRate`; `None` if either number is zero.
    pub fn rate(&self) -> Option<FeeRate> {
        FeeRate::new(self.per_weight, self.mask)
    }
}

#[derive(Deserialize, Debug)]
pub struct EmptyResponse {}
#[derive(Deserialize, Debug)]
pub struct JsonRpcResponse<T> {
    result: T,
}

#[derive(Deserialize, Debug)]
struct TransactionResponse {
    tx_hash: String,
    as_hex: String,
    pruned_as_hex: String,
    #[serde(default)]
    prunable_hash: String,
}
#[derive(Deserialize, Debug)]
struct TransactionsResponse {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    missed_tx: Vec<String>,
    txs: Vec<TransactionResponse>,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
struct GetOutputsOut {
    amount: u64,
    index: u64,
}

#[derive(Serialize, Deserialize, Debug)]
struct GetOutputsRequest {
    client: String,
    get_txid: bool,
    outputs: Vec<GetOutputsOut>,
}

#[derive(Serialize, Deserialize, Debug)]
struct GetOutputsResponse {
    outs: Vec<GetOutputsKey>,
    status: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct GetOutputsKey {
    #[serde(with = "epee_byte_blob")]
    key: Vec<u8>,
    #[serde(with = "epee_byte_blob")]
    mask: Vec<u8>,
    unlocked: bool,
    height: u64,
    #[serde(with = "epee_byte_blob")]
    txid: Vec<u8>,
}

#[derive(Serialize, Debug)]
struct OutputDistributionRequest {
    amounts: Vec<u64>,
    client: String,
    compress: bool,
}

#[derive(Deserialize, Debug)]
struct OutputDistributionResponse {
    distributions: Vec<CompressedOutputDistribution>,
    status: String,
}

#[derive(Deserialize, Debug)]
struct CompressedOutputDistribution {
    amount: u64,
    start_height: u64,
    binary: bool,
    compress: bool,
    #[serde(with = "epee_byte_blob")]
    compressed_data: Vec<u8>,
    base: u64,
}

/// The quantization mask monerod reports; a larger one is refused under any
/// fee limit.
pub const MAX_DAEMON_FEE_QUANTIZATION_MASK: u64 = 10_000;

/// Why a call to the node failed.
#[derive(Clone, PartialEq, Eq, Debug, Error)]
pub enum RpcError {
    /// A reply that could not be read, or a request that could not be written.
    #[error("internal error ({0})")]
    InternalError(&'static str),
    #[error("connection error")]
    ConnectionError,
    #[error("invalid node")]
    InvalidNode,
    #[error("unsupported protocol version ({0})")]
    UnsupportedProtocol(usize),
    #[error("transactions not found")]
    TransactionsNotFound(Vec<[u8; 32]>),
    #[error("invalid point ({0})")]
    InvalidPoint(String),
    /// The node sent the transaction pruned.
    #[error(
        "transaction {} is pruned on this node; ask an unpruned node, or read it \
         with a call that accepts pruned transactions",
        hex::encode(.0)
    )]
    PrunedTransaction([u8; 32]),
    #[error("invalid transaction ({0:?})")]
    InvalidTransaction([u8; 32]),
    #[error("fee exceeds limit (per_weight: {per_weight}, limit: {limit})")]
    FeeExceedsLimit { per_weight: u64, limit: u64 },
    #[error("zero fee returned by node")]
    ZeroFee,
    #[error("zero fee mask returned by node")]
    ZeroMask,
    #[error("fee mask exceeds limit (mask: {mask}, limit: {limit})")]
    MaskExceedsLimit { mask: u64, limit: u64 },
    /// The reply's status was not "OK".
    #[error("node answered with status {0:?}")]
    NodeStatus(String),
    #[error("invalid fee estimate ({0})")]
    InvalidFeeEstimate(&'static str),
    #[error("reply exceeds the {limit} bytes allowed for this call")]
    ResponseTooLarge { limit: usize },
    /// The node answered with an HTTP status other than 2xx.
    #[error("node answered HTTP {status}{}", body_suffix(.body))]
    HttpStatus { status: u16, body: String },
    /// The node answered a JSON-RPC call with an error object.
    #[error("node refused the call with JSON-RPC error {code} ({message:?})")]
    JsonRpcError { code: i64, message: String },
    /// Not sent: monerod drops a request over 1 MiB without a reply.
    #[error("request of {size} bytes exceeds the {limit} bytes a node accepts")]
    RequestTooLarge { size: usize, limit: usize },
}

/// The most characters of node-supplied text an error keeps.
pub const MAX_ERROR_TEXT: usize = 128;

/// The largest request body sent: monerod's 1 MiB limit less 4 KiB for headers.
pub const MAX_REQUEST_BYTES: usize = 1_048_576 - 4096;

const ERROR_BODY_BYTES: usize = 4 * MAX_ERROR_TEXT;

fn body_suffix(body: &str) -> String {
    if body.is_empty() {
        String::new()
    } else {
        format!(" ({body:?})")
    }
}

fn bounded_text(text: &[u8]) -> String {
    let head = &text[..text.len().min(4 * MAX_ERROR_TEXT)];
    let text: String = String::from_utf8_lossy(head)
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_ERROR_TEXT)
        .collect();
    text.trim().to_string()
}

impl RpcError {
    /// An `HttpStatus` error with `body` cut to `MAX_ERROR_TEXT` characters.
    pub fn http_status(status: u16, body: &[u8]) -> Self {
        RpcError::HttpStatus {
            status,
            body: bounded_text(body),
        }
    }

    /// A `NodeStatus` error with `status` cut to `MAX_ERROR_TEXT` characters.
    pub fn node_status(status: &str) -> Self {
        RpcError::NodeStatus(bounded_text(status.as_bytes()))
    }
}

fn json_refusal(reply: &[u8]) -> Option<RpcError> {
    #[derive(Deserialize)]
    struct ErrorObject {
        #[serde(default)]
        code: i64,
        #[serde(default)]
        message: String,
    }
    #[derive(Deserialize)]
    struct Status {
        #[serde(default)]
        status: Option<String>,
    }
    #[derive(Deserialize)]
    struct Envelope {
        #[serde(default)]
        status: Option<String>,
        #[serde(default)]
        error: Option<ErrorObject>,
        #[serde(default)]
        result: Option<Status>,
    }
    let envelope: Envelope = serde_json::from_slice(reply).ok()?;
    if let Some(error) = envelope.error {
        return Some(RpcError::JsonRpcError {
            code: error.code,
            message: bounded_text(error.message.as_bytes()),
        });
    }
    [
        envelope.status,
        envelope.result.and_then(|result| result.status),
    ]
    .into_iter()
    .flatten()
    .find(|status| status != "OK")
    .map(|status| RpcError::node_status(&status))
}

fn binary_refusal(reply: &[u8]) -> Option<RpcError> {
    #[derive(Deserialize)]
    struct Status {
        #[serde(default)]
        status: String,
    }
    let reply: Status = crate::safe_epee::from_bytes(reply).ok()?;
    (!reply.status.is_empty() && reply.status != "OK").then(|| RpcError::node_status(&reply.status))
}

/// The highest fee per byte a correct daemon can report for each tier; a rate
/// above it is refused under any limit.
pub const MAX_DAEMON_FEE_PER_BYTE: [u64; 4] = [1_200_000, 4_700_000, 19_000_000, 240_000_000];

/// The default fee-per-byte limit: the highest tier bound.
pub const DEFAULT_MAX_FEE_PER_BYTE: u64 = MAX_DAEMON_FEE_PER_BYTE[3];

/// Require a nondecreasing cumulative RingCT output distribution.
///
/// Decoy selection subtracts adjacent entries to count outputs per block and
/// uses an earlier cumulative count to bound eligible output indices. A decrease
/// would underflow that subtraction and invalidate the sampling bound.
fn check_distribution_is_monotonic(distribution: &[u64]) -> Result<(), RpcError> {
    let mut previous = 0u64;
    for entry in distribution {
        if *entry < previous {
            return Err(RpcError::InvalidNode);
        }
        previous = *entry;
    }
    Ok(())
}

fn require_ok_status(status: &str) -> Result<(), RpcError> {
    if status == "OK" {
        Ok(())
    } else {
        Err(RpcError::node_status(status))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct BacklogEntry {
    weight: u64,
    fee: u64,
}

fn epee_string(body: &[u8], quote: usize) -> Result<(Vec<u8>, usize), RpcError> {
    let mut bytes = Vec::new();
    let mut at = quote + 1;
    loop {
        match *body.get(at).ok_or(RpcError::InvalidNode)? {
            b'"' => return Ok((bytes, at)),
            b'\\' => {
                at += 1;
                match *body.get(at).ok_or(RpcError::InvalidNode)? {
                    b'b' => bytes.push(0x08),
                    b'f' => bytes.push(0x0c),
                    b'n' => bytes.push(b'\n'),
                    b'r' => bytes.push(b'\r'),
                    b't' => bytes.push(b'\t'),
                    b'v' => bytes.push(0x0b),
                    literal @ (b'"' | b'\\' | b'/' | b'\'') => bytes.push(literal),
                    b'u' => {
                        let mut code = 0u32;
                        for digit in body.get(at + 1..at + 5).ok_or(RpcError::InvalidNode)? {
                            let digit = char::from(*digit)
                                .to_digit(16)
                                .ok_or(RpcError::InvalidNode)?;
                            code = (code << 4) | digit;
                        }
                        let low = |shift: u32| 0x80 | ((code >> shift) & 0x3f) as u8;
                        match code {
                            0..=0x7f => bytes.push(code as u8),
                            0x80..=0x7ff => bytes.extend([0xc0 | (code >> 6) as u8, low(0)]),
                            _ => bytes.extend([0xe0 | (code >> 12) as u8, low(6), low(0)]),
                        }
                        at += 4;
                    }
                    _ => return Err(RpcError::InvalidNode),
                }
            }
            byte => bytes.push(byte),
        }
        at += 1;
    }
}

/// Decode monerod's `get_txpool_backlog` reply as epee's reader reads it:
/// `backlog` is one string of raw 24-byte records that may hold any byte.
fn parse_txpool_backlog(body: &[u8]) -> Result<Vec<BacklogEntry>, RpcError> {
    const RECORD: usize = 24;

    let mut blob = Vec::new();
    let mut json = Vec::with_capacity(body.len());
    let mut backlog_value_next = false;
    let mut backlog_seen = false;
    let mut at = 0;
    while let Some(&byte) = body.get(at) {
        if byte != b'"' {
            // wallet2 reads a `backlog` that is not a string as empty.
            if !byte.is_ascii_whitespace() && byte != b':' {
                backlog_value_next = false;
            }
            json.push(byte);
            at += 1;
            continue;
        }
        let (bytes, end) = epee_string(body, at)?;
        at = end + 1;
        let mut after = at;
        while body.get(after).is_some_and(u8::is_ascii_whitespace) {
            after += 1;
        }
        let name = body.get(after) == Some(&b':');
        if name && !backlog_seen && bytes == b"backlog" {
            backlog_seen = true;
            backlog_value_next = true;
        } else if !name && backlog_value_next {
            backlog_value_next = false;
            blob = bytes;
            json.extend(b"\"\"");
            continue;
        }
        json.extend(
            serde_json::to_vec(&String::from_utf8_lossy(&bytes))
                .map_err(|_| RpcError::InternalError("Failed to serialize JSON string"))?,
        );
    }

    #[derive(Deserialize, Debug)]
    struct Status {
        #[serde(default)]
        status: String,
    }
    let reply: JsonRpcResponse<Status> = serde_json::from_slice(&json).map_err(|_| {
        json_refusal(&json).unwrap_or(RpcError::InternalError("Failed to parse JSON response"))
    })?;
    require_ok_status(&reply.result.status)?;

    // wallet2 reads a partial record as an empty backlog.
    if blob.len() % RECORD != 0 {
        return Ok(vec![]);
    }
    Ok(blob
        .chunks_exact(RECORD)
        .map(|record| {
            let word = |at: usize| {
                u64::from_le_bytes(record[at..at + 8].try_into().expect("an eight-byte slice"))
            };
            BacklogEntry {
                weight: word(0),
                fee: word(8),
            }
        })
        .collect())
}

fn rpc_hex(value: &str) -> Result<Vec<u8>, RpcError> {
    hex::decode(value).map_err(|_| RpcError::InvalidNode)
}

fn hash_hex(hash: &str) -> Result<[u8; 32], RpcError> {
    rpc_hex(hash)?.try_into().map_err(|_| RpcError::InvalidNode)
}

fn rpc_point(point: &str) -> Result<EdwardsPoint, RpcError> {
    CompressedEdwardsY(
        rpc_hex(point)?
            .try_into()
            .map_err(|_| RpcError::InvalidPoint(point.to_string()))?,
    )
    .decompress()
    .ok_or_else(|| RpcError::InvalidPoint(point.to_string()))
}

#[cfg(not(target_arch = "wasm32"))]
#[async_trait]
pub trait RpcConnection: Clone + Debug + Send + Sync {
    /// Perform a POST request to the specified route with the specified body.
    ///
    /// The implementor is left to handle anything such as authentication.
    async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError>;

    /// POST whose reply must fit in `limit` bytes. Override to stop reading
    /// early; the default reads the whole reply and then checks its size.
    async fn post_limited(
        &self,
        route: &str,
        body: Vec<u8>,
        limit: usize,
    ) -> Result<Vec<u8>, RpcError> {
        let reply = self.post(route, body).await?;
        if reply.len() > limit {
            return Err(RpcError::ResponseTooLarge { limit });
        }
        Ok(reply)
    }
}

#[cfg(target_arch = "wasm32")]
#[async_trait(?Send)]
pub trait RpcConnection: Clone + Debug {
    /// Perform a POST request to the specified route with the specified body.
    ///
    /// The implementor is left to handle anything such as authentication.
    async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError>;

    /// POST whose reply must fit in `limit` bytes. Override to stop reading
    /// early; the default reads the whole reply and then checks its size.
    async fn post_limited(
        &self,
        route: &str,
        body: Vec<u8>,
        limit: usize,
    ) -> Result<Vec<u8>, RpcError> {
        let reply = self.post(route, body).await?;
        if reply.len() > limit {
            return Err(RpcError::ResponseTooLarge { limit });
        }
        Ok(reply)
    }
}

/// Time limits of the built-in HTTP transport.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HttpTimeouts {
    /// Time allowed to establish a connection.
    pub connect: Duration,
    /// Longest silence tolerated while waiting for or receiving a reply.
    pub idle: Duration,
    /// Time allowed for a whole call, before the allowance for its size.
    pub total: Duration,
    /// Slowest average transfer rate tolerated: a call may also take as long
    /// as its reply limit needs at this rate.
    pub min_bytes_per_sec: u64,
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for HttpTimeouts {
    fn default() -> Self {
        HttpTimeouts {
            connect: Duration::from_secs(30),
            idle: Duration::from_secs(210),
            total: Duration::from_secs(210),
            min_bytes_per_sec: 16 * 1024,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl HttpTimeouts {
    fn deadline(&self, limit: usize) -> Duration {
        let transfer = (limit as u64) / self.min_bytes_per_sec.max(1);
        self.total.saturating_add(Duration::from_secs(transfer))
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug)]
pub struct HttpRpc {
    client: Client,
    userpass: Option<(String, String)>,
    url: String,
    host: Option<String>,
    timeouts: HttpTimeouts,
}

#[cfg(not(target_arch = "wasm32"))]
impl HttpRpc {
    /// Create a new HTTP(S) RPC connection, with credentials in the URL if the
    /// daemon requires them.
    pub fn new(url: String) -> Result<Rpc<HttpRpc>, RpcError> {
        Self::with_timeouts(url, HttpTimeouts::default())
    }

    /// Create a new HTTP(S) RPC connection with explicit time limits.
    pub fn with_timeouts(
        mut url: String,
        timeouts: HttpTimeouts,
    ) -> Result<Rpc<HttpRpc>, RpcError> {
        // Parse out the username and password
        let userpass = if url.contains('@') {
            let url_clone = url;
            let split_url = url_clone.split('@').collect::<Vec<_>>();
            if split_url.len() != 2 {
                Err(RpcError::InvalidNode)?;
            }
            let mut userpass = split_url[0];
            url = split_url[1].to_string();

            // If there was additionally a protocol string, restore that to the daemon URL
            if userpass.contains("://") {
                let split_userpass = userpass.split("://").collect::<Vec<_>>();
                if split_userpass.len() != 2 {
                    Err(RpcError::InvalidNode)?;
                }
                url = split_userpass[0].to_string() + "://" + &url;
                userpass = split_userpass[1];
            }

            let split_userpass = userpass.split(':').collect::<Vec<_>>();
            if split_userpass.len() != 2 {
                Err(RpcError::InvalidNode)?;
            }
            Some((split_userpass[0].to_string(), split_userpass[1].to_string()))
        } else {
            None
        };

        let client = Client::builder()
            .connect_timeout(timeouts.connect)
            .read_timeout(timeouts.idle)
            .http1_title_case_headers()
            .build()
            .map_err(|_| RpcError::InternalError("Failed to build the HTTP client"))?;
        let host = reqwest::Url::parse(&url)
            .ok()
            .and_then(|parsed| parsed.host_str().map(str::to_string));
        Ok(Rpc::new_with_connection(HttpRpc {
            client,
            userpass,
            url,
            host,
            timeouts,
        }))
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[async_trait]
impl RpcConnection for HttpRpc {
    async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
        self.post_limited(route, body, MAX_RESPONSE_BYTES).await
    }

    async fn post_limited(
        &self,
        route: &str,
        body: Vec<u8>,
        limit: usize,
    ) -> Result<Vec<u8>, RpcError> {
        let attempts = if is_read_only(route, &body) {
            1 + CONNECTION_RETRIES
        } else {
            1
        };
        let mut attempt = 1;
        loop {
            match self.attempt(route, body.clone(), limit).await {
                Err(Failure::Connection) if attempt < attempts => attempt += 1,
                Err(Failure::Connection) => return Err(RpcError::ConnectionError),
                Err(Failure::Final(error)) => return Err(error),
                Ok(reply) => return Ok(reply),
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
const CONNECTION_RETRIES: usize = 2;

#[cfg(not(target_arch = "wasm32"))]
enum Failure {
    Connection,
    Final(RpcError),
}

#[cfg(not(target_arch = "wasm32"))]
impl From<reqwest::Error> for Failure {
    fn from(error: reqwest::Error) -> Self {
        if error.is_timeout() {
            Failure::Final(RpcError::ConnectionError)
        } else {
            Failure::Connection
        }
    }
}

/// Whether sending the request again is harmless.
#[cfg(not(target_arch = "wasm32"))]
fn is_read_only(route: &str, body: &[u8]) -> bool {
    match route {
        "get_height"
        | "getblocks.bin"
        | "gethashes.bin"
        | "get_outs.bin"
        | "get_output_distribution.bin"
        | "get_o_indexes.bin"
        | "get_transactions"
        | "gettransactions"
        | "get_transaction_pool"
        | "get_transaction_pool_hashes.bin"
        | "is_key_image_spent" => true,
        "json_rpc" => serde_json::from_slice::<Value>(body)
            .ok()
            .and_then(|request| {
                request.get("method").and_then(Value::as_str).map(|method| {
                    matches!(
                        method,
                        "get_info"
                            | "get_fee_estimate"
                            | "get_txpool_backlog"
                            | "getblockheadersrange"
                            | "get_last_block_header"
                            | "get_block_header_by_height"
                            | "get_block"
                            | "hard_fork_info"
                    )
                })
            })
            .unwrap_or(false),
        _ => false,
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl HttpRpc {
    async fn attempt(&self, route: &str, body: Vec<u8>, limit: usize) -> Result<Vec<u8>, Failure> {
        fn invalid<E>(_: E) -> Failure {
            Failure::Final(RpcError::InvalidNode)
        }
        // wallet2 sends Host, Content-Length and, on JSON routes, Content-Type,
        // in that order; reqwest also adds `Accept: */*`.
        let mut builder = self
            .client
            .post(self.url.clone() + "/" + route)
            .timeout(self.timeouts.deadline(limit));
        if let Some(host) = &self.host {
            builder = builder.header(reqwest::header::HOST, host);
        }
        builder = builder.header(reqwest::header::CONTENT_LENGTH, body.len());
        if !is_binary_route(route) {
            builder = builder.header(
                reqwest::header::CONTENT_TYPE,
                "application/json; charset=utf-8",
            );
        }
        let mut builder = builder.body(body);

        if let Some((user, pass)) = &self.userpass {
            let req = self
                .client
                .post(&self.url)
                .timeout(self.timeouts.total)
                .send()
                .await
                .map_err(invalid)?;
            // Only provide authentication if this daemon actually expects it
            if let Some(header) = req.headers().get("www-authenticate") {
                builder = builder.header(
                    "Authorization",
                    digest_auth::parse(header.to_str().map_err(invalid)?)
                        .map_err(invalid)?
                        .respond(&AuthContext::new_post::<_, _, _, &[u8]>(
                            user,
                            pass,
                            "/".to_string() + route,
                            None,
                        ))
                        .map_err(invalid)?
                        .to_header_string(),
                );
            }
        }

        let mut response = builder.send().await?;
        let status = response.status();
        if !status.is_success() {
            let mut excerpt = Vec::new();
            while excerpt.len() < ERROR_BODY_BYTES {
                match response.chunk().await {
                    Ok(Some(chunk)) => excerpt.extend_from_slice(&chunk),
                    Ok(None) | Err(_) => break,
                }
            }
            excerpt.truncate(ERROR_BODY_BYTES);
            return Err(Failure::Final(RpcError::http_status(
                status.as_u16(),
                &excerpt,
            )));
        }
        let too_large = Failure::Final(RpcError::ResponseTooLarge { limit });
        if response
            .content_length()
            .is_some_and(|length| length > limit as u64)
        {
            return Err(too_large);
        }
        let mut reply = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if reply.len() + chunk.len() > limit {
                return Err(too_large);
            }
            reply.extend_from_slice(&chunk);
        }
        Ok(reply)
    }
}

/// Whether `route` is epee binary. get_transaction_pool_hashes.bin is JSON:
/// wallet2 posts it with invoke_http_json.
fn is_binary_route(route: &str) -> bool {
    route.ends_with(".bin") && route != "get_transaction_pool_hashes.bin"
}

fn route_carries_client(route: &str) -> bool {
    matches!(
        route,
        "get_transactions"
            | "gettransactions"
            | "is_key_image_spent"
            | "send_raw_transaction"
            | "sendrawtransaction"
            | "get_transaction_pool"
            | "get_transaction_pool_hashes"
            | "get_transaction_pool_hashes.bin"
            | "get_transaction_pool_stats"
            | "get_outs"
            | "get_output_distribution"
    )
}

fn method_carries_client(method: &str) -> bool {
    matches!(
        method,
        "get_info"
            | "get_fee_estimate"
            | "get_txpool_backlog"
            | "getblockheadersrange"
            | "get_block_headers_range"
            | "get_last_block_header"
            | "getlastblockheader"
            | "get_block_header_by_height"
            | "getblockheaderbyheight"
            | "get_block_header_by_hash"
            | "getblockheaderbyhash"
            | "get_block"
            | "getblock"
            | "hard_fork_info"
            | "get_coinbase_tx_sum"
            | "get_output_histogram"
            | "get_output_distribution"
            | "relay_tx"
            | "sync_info"
    )
}

fn json_rpc_request(method: &str, params: Value) -> Value {
    json!({ "id": "0", "jsonrpc": "2.0", "method": method, "params": params })
}

fn epee_json(value: &Value) -> Vec<u8> {
    let mut out = String::new();
    write_epee_json(&mut out, value, 0);
    out.into_bytes()
}

fn write_epee_json(out: &mut String, value: &Value, indent: usize) {
    match value {
        Value::Object(members) => {
            out.push_str("{\r\n");
            let mut members: Vec<_> = members.iter().collect();
            members.sort_by(|a, b| a.0.cmp(b.0));
            let count = members.len();
            for (position, (name, member)) in members.into_iter().enumerate() {
                out.push_str(&"  ".repeat(indent + 1));
                write_epee_string(out, name);
                out.push_str(": ");
                write_epee_json(out, member, indent + 1);
                if position + 1 != count {
                    out.push(',');
                }
                out.push_str("\r\n");
            }
            out.push_str(&"  ".repeat(indent));
            out.push('}');
        }
        Value::Array(elements) => {
            out.push('[');
            for (position, element) in elements.iter().enumerate() {
                if position != 0 {
                    out.push(',');
                }
                write_epee_json(out, element, indent);
            }
            out.push(']');
        }
        Value::String(text) => write_epee_string(out, text),
        Value::Number(number) => out.push_str(&number.to_string()),
        Value::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Value::Null => out.push_str("null"),
    }
}

fn write_epee_string(out: &mut String, text: &str) {
    out.push('"');
    for character in text.chars() {
        match character {
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{b}' => out.push_str("\\v"),
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '/' => out.push_str("\\/"),
            other => out.push(other),
        }
    }
    out.push('"');
}

fn epee_json_string(body: &[u8], name: &str) -> Option<Vec<u8>> {
    let key = format!("\"{name}\": \"");
    let start = body
        .windows(key.len())
        .position(|window| window == key.as_bytes())?
        + key.len();
    let mut out = Vec::new();
    let mut bytes = body[start..].iter();
    loop {
        match *bytes.next()? {
            b'"' => return Some(out),
            b'\\' => out.push(match *bytes.next()? {
                b'b' => 8,
                b'f' => 12,
                b'n' => b'\n',
                b'r' => b'\r',
                b't' => b'\t',
                b'v' => 11,
                other => other,
            }),
            byte => out.push(byte),
        }
    }
}

const MIB: usize = 1 << 20;

/// The largest reply of any call that carries no pool data.
pub const MAX_RESPONSE_BYTES: usize = 256 * MIB;

/// The reply limit of a getblocks.bin call that also asks for the pool.
pub const MAX_POOL_REPLY_BYTES: usize = 768 * MIB;

/// `MAX_POOL_REPLY_BYTES` when pruned data is asked for.
pub const MAX_PRUNED_POOL_REPLY_BYTES: usize = 512 * MIB;

fn response_limit(route: &str, method: Option<&str>) -> usize {
    match (route, method) {
        ("getblocks.bin", _) | ("get_transaction_pool", _) | ("get_transactions", _) => {
            MAX_RESPONSE_BYTES
        }
        ("get_output_distribution.bin", _)
        | ("get_transaction_pool_hashes.bin", _)
        | ("json_rpc", Some("get_txpool_backlog")) => 64 * MIB,
        ("get_height", _)
        | ("send_raw_transaction", _)
        | ("sendrawtransaction", _)
        | ("get_o_indexes.bin", _)
        | ("gethashes.bin", _)
        | (
            "json_rpc",
            Some(
                "get_last_block_header"
                | "get_block_header_by_height"
                | "hard_fork_info"
                | "get_fee_estimate"
                | "get_info"
                | "getblockheadersrange",
            ),
        ) => MIB,
        _ => 16 * MIB,
    }
}

const NODE_CACHE_SECONDS: u64 = 30;

#[derive(Clone, PartialEq, Eq, Debug)]
struct FeeEstimate {
    mask: u64,
    tiers: Vec<u64>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct NodeInfo {
    height: u64,
    block_weight_limit: u64,
}

#[derive(Debug, Default)]
struct NodeCacheState {
    fee_estimate: Option<(Option<u64>, Option<u64>, FeeEstimate)>,
    wallet_height: Option<(u64, u64)>,
    info: Option<(u64, NodeInfo)>,
    fork: Option<ForkInfo>,
    genesis: Option<[u8; 32]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ForkInfo {
    version: usize,
    plus_from: u64,
    at: Option<u64>,
}

impl ForkInfo {
    fn version_at(&self, wallet_height: Option<u64>) -> usize {
        if self.version < 15 && wallet_height.is_some_and(|height| height >= self.plus_from) {
            15
        } else {
            self.version
        }
    }

    fn decides(&self, wallet_height: Option<u64>, now: Option<u64>) -> bool {
        self.version_at(wallet_height) >= 15 || self.at.is_some_and(|at| is_recent(at, now))
    }
}

fn is_recent(at: u64, now: Option<u64>) -> bool {
    now.is_some_and(|now| now >= at && now - at < NODE_CACHE_SECONDS)
}

impl NodeCacheState {
    fn fee_estimate(&self, wallet_height: Option<u64>, now: Option<u64>) -> Option<FeeEstimate> {
        let (height, at, estimate) = self.fee_estimate.as_ref()?;
        let current = *height == wallet_height
            && (wallet_height.is_some() || at.is_some_and(|at| is_recent(at, now)));
        current.then(|| estimate.clone())
    }

    fn wallet_height_is_current(&mut self, wallet_height: u64, now: Option<u64>) -> bool {
        match self.wallet_height {
            Some((height, since)) if height == wallet_height => is_recent(since, now),
            _ => {
                self.wallet_height = now.map(|now| (wallet_height, now));
                now.is_some()
            }
        }
    }

    fn info(&self, now: Option<u64>) -> Option<NodeInfo> {
        self.info
            .filter(|(at, _)| is_recent(*at, now))
            .map(|(_, info)| info)
    }
}

#[derive(Clone, Debug, Default)]
struct NodeCache {
    state: Arc<Mutex<NodeCacheState>>,
    by_client: Arc<Mutex<std::collections::HashMap<[u8; 32], Arc<Mutex<NodeCacheState>>>>>,
}

impl NodeCache {
    /// Never held across an await.
    fn state(&self) -> MutexGuard<'_, NodeCacheState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn for_client(&self, client: &RpcClientKey) -> Self {
        let state = self
            .by_client
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(client.public_key())
            .or_default()
            .clone();
        NodeCache {
            state,
            by_client: self.by_client.clone(),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn client_key_for_url(url: &str) -> RpcClientKey {
    static KEYS: std::sync::OnceLock<Mutex<std::collections::HashMap<String, RpcClientKey>>> =
        std::sync::OnceLock::new();
    let mut keys = KEYS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if keys.len() >= 64 && !keys.contains_key(url) {
        keys.clear();
    }
    keys.entry(url.to_string())
        .or_insert_with(RpcClientKey::random)
        .clone()
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn url_rpc(url: &str) -> Result<Rpc<HttpRpc>, RpcError> {
    Ok(HttpRpc::new(url.to_string())?.with_client_key(&client_key_for_url(url)))
}

/// A node connection. Clones share what the node answered recently.
#[derive(Clone, Debug)]
pub struct Rpc<R: RpcConnection>(R, NodeCache, RpcClientKey);
impl<R: RpcConnection> Rpc<R> {
    /// Create a new Rpc instance from any RpcConnection implementation. It signs
    /// with a random key: use [`Self::for_wallet`] for a wallet's requests.
    pub fn new_with_connection(connection: R) -> Self {
        let key = RpcClientKey::random();
        Rpc(connection, NodeCache::default().for_client(&key), key)
    }

    /// The same connection, signing with `key`.
    pub fn with_client_key(&self, key: &RpcClientKey) -> Self {
        Rpc(self.0.clone(), self.1.for_client(key), key.clone())
    }

    /// The same connection, signing with the key of `state`'s wallet.
    pub fn for_wallet(&self, state: &crate::wallet_state::WalletState) -> Self {
        self.with_client_key(state.rpc_client_key())
    }

    /// The key requests are signed with.
    pub fn client_key(&self) -> &RpcClientKey {
        &self.2
    }

    async fn send(&self, route: &str, body: Vec<u8>, limit: usize) -> Result<Vec<u8>, RpcError> {
        if body.len() > MAX_REQUEST_BYTES {
            return Err(RpcError::RequestTooLarge {
                size: body.len(),
                limit: MAX_REQUEST_BYTES,
            });
        }
        self.0.post_limited(route, body, limit).await
    }

    fn client(&self) -> String {
        self.2.signature()
    }

    fn with_client(&self, method: &str, params: Option<Value>) -> Value {
        let mut params = params.unwrap_or_else(|| json!({}));
        if method_carries_client(method) {
            if let Value::Object(members) = &mut params {
                members
                    .entry("client")
                    .or_insert_with(|| Value::String(self.client()));
            }
        }
        params
    }

    /// Perform a RPC call to the specified route with the provided parameters.
    ///
    /// This is NOT a JSON-RPC call. They use a route of "json_rpc" and are
    /// available via `json_rpc_call`.
    pub async fn rpc_call<Params: Serialize + Debug, Response: DeserializeOwned + Debug>(
        &self,
        route: &str,
        params: Option<Params>,
    ) -> Result<Response, RpcError> {
        self.rpc_call_limited(route, params, response_limit(route, None))
            .await
    }

    async fn rpc_call_limited<Params: Serialize + Debug, Response: DeserializeOwned + Debug>(
        &self,
        route: &str,
        params: Option<Params>,
        limit: usize,
    ) -> Result<Response, RpcError> {
        let mut params = match params {
            Some(params) => serde_json::to_value(&params)
                .map_err(|_| RpcError::InternalError("Failed to serialize JSON request"))?,
            None => json!({}),
        };
        if route_carries_client(route) {
            if let Value::Object(members) = &mut params {
                members
                    .entry("client")
                    .or_insert_with(|| Value::String(self.client()));
            }
        }
        self.call_tail(route, self.send(route, epee_json(&params), limit).await?)
            .await
    }

    /// Perform a JSON-RPC call with the specified method with the provided parameters.
    pub async fn json_rpc_call<Response: DeserializeOwned + Debug>(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Response, RpcError> {
        let req = json_rpc_request(method, self.with_client(method, params));
        let limit = response_limit("json_rpc", Some(method));
        Ok(self
            .rpc_call_limited::<_, JsonRpcResponse<Response>>("json_rpc", Some(req), limit)
            .await?
            .result)
    }

    /// Perform a binary call to the specified route with the provided parameters.
    pub async fn bin_call<Response: DeserializeOwned + Debug>(
        &self,
        route: &str,
        params: Vec<u8>,
    ) -> Result<Response, RpcError> {
        self.bin_call_limited(route, params, response_limit(route, None))
            .await
    }

    async fn bin_call_limited<Response: DeserializeOwned + Debug>(
        &self,
        route: &str,
        params: Vec<u8>,
        limit: usize,
    ) -> Result<Response, RpcError> {
        self.call_tail(route, self.send(route, params, limit).await?)
            .await
    }

    async fn call_tail<Response: DeserializeOwned + Debug>(
        &self,
        route: &str,
        res: Vec<u8>,
    ) -> Result<Response, RpcError> {
        Ok(if !is_binary_route(route) {
            serde_json::from_str(std::str::from_utf8(&res).map_err(|_| RpcError::InvalidNode)?)
                .map_err(|_| {
                    json_refusal(&res)
                        .unwrap_or(RpcError::InternalError("Failed to parse JSON response"))
                })?
        } else {
            crate::safe_epee::from_bytes(&res).map_err(|_| {
                binary_refusal(&res)
                    .unwrap_or(RpcError::InternalError("Failed to parse binary response"))
            })?
        })
    }

    /// The RingCT type of a transaction built under the node's current hard fork.
    pub async fn get_rct_type(&self) -> Result<RctType, RpcError> {
        self.get_rct_type_at_height(None).await
    }

    /// The RingCT type of a transaction built now by a wallet whose chain is
    /// `wallet_height` blocks long, as wallet2 decides it.
    pub async fn get_rct_type_at_height(
        &self,
        wallet_height: Option<u64>,
    ) -> Result<RctType, RpcError> {
        #[derive(Deserialize, Debug)]
        struct HardForkInfo {
            version: usize,
            #[serde(default)]
            earliest_height: Option<u64>,
            #[serde(default)]
            status: String,
        }

        let now = current_time_secs();
        let known = self
            .1
            .state()
            .fork
            .filter(|fork| fork.decides(wallet_height, now));
        let fork = match known {
            Some(fork) => fork,
            None => {
                let info: HardForkInfo = self
                    .json_rpc_call("hard_fork_info", Some(json!({ "version": 15 })))
                    .await?;
                require_ok_status(&info.status)?;
                let fork = ForkInfo {
                    version: info.version,
                    plus_from: info.earliest_height.unwrap_or(u64::MAX),
                    at: now,
                };
                self.1.state().fork = Some(fork);
                fork
            }
        };
        Ok(match fork.version_at(wallet_height) {
            13 | 14 => RctType::ClsagBulletproof,
            15 | 16 => RctType::ClsagBulletproofPlus,
            protocol => Err(RpcError::UnsupportedProtocol(protocol))?,
        })
    }

    pub async fn get_height(&self) -> Result<usize, RpcError> {
        #[derive(Deserialize, Debug)]
        struct HeightResponse {
            height: usize,
        }
        Ok(self
            .rpc_call::<Option<()>, HeightResponse>("get_height", None)
            .await?
            .height)
    }

    async fn transaction_entries(
        &self,
        hashes: &[[u8; 32]],
    ) -> Result<Vec<TransactionResponse>, RpcError> {
        let mut entries = Vec::with_capacity(hashes.len());
        let mut missing = Vec::new();
        for slice in hashes.chunks(TRANSACTIONS_PER_REQUEST) {
            let txs: TransactionsResponse = self
                .rpc_call_limited(
                    "get_transactions",
                    Some(json!({
                      "txs_hashes": slice.iter().map(hex::encode).collect::<Vec<_>>()
                    })),
                    transactions_limit(slice.len()),
                )
                .await?;
            if let Some(status) = &txs.status {
                require_ok_status(status)?;
            }

            if !txs.missed_tx.is_empty() {
                for hash in &txs.missed_tx {
                    missing.push(hash_hex(hash)?);
                }
                continue;
            }

            // One entry per hash: callers pair entries with hashes by position.
            if txs.txs.len() != slice.len() {
                return Err(RpcError::InvalidNode);
            }
            entries.extend(txs.txs);
        }
        if !missing.is_empty() {
            return Err(RpcError::TransactionsNotFound(missing));
        }
        Ok(entries)
    }

    /// Transactions by hash, in the order of `hashes`, each checked against its
    /// hash and asked for in slices a restricted node answers.
    pub async fn get_transactions(
        &self,
        hashes: &[[u8; 32]],
    ) -> Result<Vec<Transaction>, RpcError> {
        if hashes.is_empty() {
            return Ok(vec![]);
        }

        self.transaction_entries(hashes)
            .await?
            .iter()
            .enumerate()
            .map(|(i, res)| {
                let blob = rpc_hex(if !res.as_hex.is_empty() {
                    &res.as_hex
                } else {
                    &res.pruned_as_hex
                })?;
                let tx = Transaction::<NotPruned>::read::<&[u8]>(&mut blob.as_ref());

                // monerod sends a whole miner transaction in the pruned field
                // (monero-project/monero#8311); anything else there is pruned.
                let tx = match tx {
                    Ok(tx) if !res.as_hex.is_empty() => tx,
                    Ok(tx) if matches!(tx.prefix().inputs.first(), Some(Input::Gen { .. })) => tx,
                    Ok(_) => return Err(RpcError::PrunedTransaction(hashes[i])),
                    Err(_) if res.as_hex.is_empty() => {
                        return Err(pruned_entry_error(&blob, &res.prunable_hash, hashes[i]))
                    }
                    Err(_) => {
                        return Err(match hash_hex(&res.tx_hash) {
                            Ok(hash) => RpcError::InvalidTransaction(hash),
                            Err(err) => err,
                        })
                    }
                };

                // This provides resilience against invalid/malicious nodes
                if tx.hash() != hashes[i] {
                    Err(RpcError::InvalidNode)?;
                }

                Ok(tx)
            })
            .collect()
    }

    pub async fn get_transaction(&self, tx: [u8; 32]) -> Result<Transaction, RpcError> {
        self.get_transactions(&[tx])
            .await
            .map(|mut txs| txs.swap_remove(0))
    }

    /// Get timelocks for the given transaction hashes.
    /// Only parses the start of the transaction prefix, so this works with
    /// pruned node data.
    async fn get_transaction_timelocks(
        &self,
        hashes: &[[u8; 32]],
    ) -> Result<Vec<Timelock>, RpcError> {
        if hashes.is_empty() {
            return Ok(vec![]);
        }

        self.transaction_entries(hashes)
            .await?
            .iter()
            .map(|res| {
                let hex_data = if !res.as_hex.is_empty() {
                    &res.as_hex
                } else {
                    &res.pruned_as_hex
                };
                let data = rpc_hex(hex_data)?;
                prefix_timelock(&data)
            })
            .collect()
    }

    /// The id of the node's genesis block, asked for once per `Rpc`. monerod
    /// refuses, and counts against the client, a block id list that does not
    /// end in it.
    pub async fn genesis_hash(&self) -> Result<[u8; 32], RpcError> {
        if let Some(hash) = self.1.state().genesis {
            return Ok(hash);
        }
        let hash = self.get_block_hash(0).await?;
        self.1.state().genesis = Some(hash);
        Ok(hash)
    }

    /// The node's block ids from the newest id of `block_ids` it has, asked for
    /// as wallet2's `pull_hashes` asks.
    pub async fn get_hashes(
        &self,
        block_ids: &[[u8; 32]],
        start_height: u64,
    ) -> Result<BlockIds, RpcError> {
        #[derive(Serialize, Debug)]
        struct Request {
            block_ids: Vec<u8>,
            client: String,
            start_height: u64,
        }
        #[derive(Deserialize, Debug)]
        struct Response {
            status: String,
            #[serde(default, with = "epee_byte_blob")]
            m_block_ids: Vec<u8>,
            start_height: u64,
            current_height: u64,
        }
        let request = Request {
            block_ids: block_ids.iter().flatten().copied().collect(),
            client: self.client(),
            start_height,
        };
        let body = monero_epee_bin_serde::to_bytes(&request).map_err(|_| {
            RpcError::InternalError("epee serialization failed for gethashes.bin request")
        })?;
        let reply: Response = self.bin_call("gethashes.bin", body).await?;
        if reply.status != "OK" {
            return Err(RpcError::NodeStatus(reply.status));
        }
        if reply.m_block_ids.len() % 32 != 0 || reply.m_block_ids.len() / 32 > MAX_BLOCK_IDS {
            return Err(RpcError::InvalidNode);
        }
        let ids: Vec<[u8; 32]> = reply
            .m_block_ids
            .chunks(32)
            .map(|id| <[u8; 32]>::try_from(id).unwrap())
            .collect();
        if reply
            .start_height
            .checked_add(ids.len() as u64)
            .map_or(true, |end| end > reply.current_height)
        {
            return Err(RpcError::InvalidNode);
        }
        Ok(BlockIds {
            start_height: reply.start_height,
            current_height: reply.current_height,
            ids,
        })
    }

    /// Get the hash of a block from the node by the block's number.
    /// This function does not verify the returned block hash is actually for
    /// the number in question.
    pub async fn get_block_hash(&self, number: usize) -> Result<[u8; 32], RpcError> {
        #[derive(Deserialize, Debug)]
        struct BlockHeaderResponse {
            hash: String,
        }
        #[derive(Deserialize, Debug)]
        struct BlockHeaderByHeightResponse {
            block_header: BlockHeaderResponse,
        }

        let header: BlockHeaderByHeightResponse = self
            .json_rpc_call(
                "get_block_header_by_height",
                Some(json!({ "height": number })),
            )
            .await?;
        rpc_hex(&header.block_header.hash)?
            .try_into()
            .map_err(|_| RpcError::InvalidNode)
    }

    /// Fetches a block by hash and verifies its content commitment.
    pub async fn get_block(&self, hash: [u8; 32]) -> Result<Block, RpcError> {
        #[derive(Deserialize, Debug)]
        struct BlockResponse {
            blob: String,
        }

        let res: BlockResponse = self
            .json_rpc_call("get_block", Some(json!({ "hash": hex::encode(hash) })))
            .await?;

        let block = Block::read::<&[u8]>(&mut rpc_hex(&res.blob)?.as_ref())
            .map_err(|_| RpcError::InvalidNode)?;

        // Verify the block ID matches the requested hash
        if block.hash() != hash {
            return Err(RpcError::InvalidNode);
        }

        Ok(block)
    }

    pub async fn get_block_by_number(&self, number: usize) -> Result<Block, RpcError> {
        let block = self.get_block(self.get_block_hash(number).await?).await?;
        // Make sure this is actually the block for this number
        if block.number() == number {
            Ok(block)
        } else {
            Err(RpcError::InvalidNode)
        }
    }

    pub async fn get_block_transactions(
        &self,
        hash: [u8; 32],
    ) -> Result<Vec<Transaction>, RpcError> {
        let block = self.get_block(hash).await?;
        let mut res = vec![block.miner_transaction().clone()];
        res.extend(self.get_transactions(&block.transactions).await?);
        Ok(res)
    }

    pub async fn get_block_transactions_by_number(
        &self,
        number: usize,
    ) -> Result<Vec<Transaction>, RpcError> {
        self.get_block_transactions(self.get_block_hash(number).await?)
            .await
    }

    /// Get the output indexes of the specified transaction.
    pub async fn get_o_indexes(&self, hash: [u8; 32]) -> Result<Vec<u64>, RpcError> {
        #[derive(Serialize, Debug)]
        struct Request {
            client: String,
            txid: [u8; 32],
        }

        #[allow(dead_code)]
        #[derive(Deserialize, Debug)]
        struct OIndexes {
            o_indexes: Vec<u64>,
            status: String,
            untrusted: bool,
            credits: usize,
            top_hash: String,
        }

        let indexes: OIndexes = self
            .bin_call(
                "get_o_indexes.bin",
                monero_epee_bin_serde::to_bytes(&Request {
                    client: self.client(),
                    txid: hash,
                })
                .map_err(|_| {
                    RpcError::InternalError("epee serialization failed for get_o_indexes request")
                })?,
            )
            .await?;

        require_ok_status(&indexes.status)?;
        Ok(indexes.o_indexes)
    }

    /// Get the output distribution, from the specified height to the
    /// specified height (both inclusive).
    pub async fn get_output_distribution(
        &self,
        _from: usize,
        _to: usize,
    ) -> Result<Vec<u64>, RpcError> {
        Ok(self.get_rct_output_distribution().await?.1)
    }

    pub(crate) async fn get_rct_output_distribution(&self) -> Result<(u64, Vec<u64>), RpcError> {
        let response: OutputDistributionResponse = self
            .bin_call(
                "get_output_distribution.bin",
                monero_epee_bin_serde::to_bytes(&OutputDistributionRequest {
                    amounts: vec![0],
                    client: self.client(),
                    compress: true,
                })
                .map_err(|_| {
                    RpcError::InternalError(
                        "epee serialization failed for output distribution request",
                    )
                })?,
            )
            .await?;
        require_ok_status(&response.status)?;
        let [distribution] = response.distributions.as_slice() else {
            return Err(RpcError::InvalidNode);
        };
        if distribution.amount != 0 || !distribution.binary || !distribution.compress {
            return Err(RpcError::InvalidNode);
        }

        let mut position = 0usize;
        let mut cumulative = distribution.base;
        let mut result = Vec::with_capacity(distribution.compressed_data.len());
        while position < distribution.compressed_data.len() {
            let mut value = 0u64;
            let mut shift = 0u32;
            loop {
                let byte = *distribution
                    .compressed_data
                    .get(position)
                    .ok_or(RpcError::InvalidNode)?;
                position += 1;
                value |= u64::from(byte & 0x7f)
                    .checked_shl(shift)
                    .ok_or(RpcError::InvalidNode)?;
                if byte & 0x80 == 0 {
                    break;
                }
                shift += 7;
                if shift >= 64 {
                    return Err(RpcError::InvalidNode);
                }
            }
            cumulative = cumulative.checked_add(value).ok_or(RpcError::InvalidNode)?;
            result.push(cumulative);
        }
        check_distribution_is_monotonic(&result)?;
        Ok((distribution.start_height, result))
    }

    /// Get the specified outputs from the RingCT (zero-amount) pool, but only
    /// return them if their timelock has been satisfied. This is distinct
    /// from being free of the 10-block lock applied to all Monero
    /// transactions.
    pub async fn get_unlocked_outputs(
        &self,
        indexes: &[u64],
        height: usize,
    ) -> Result<Vec<Option<[EdwardsPoint; 2]>>, RpcError> {
        const WALLET2_GET_OUTS_CHUNK: usize = 1000;
        let mut result = Vec::with_capacity(indexes.len());
        for indexes in indexes.chunks(WALLET2_GET_OUTS_CHUNK) {
            let request = GetOutputsRequest {
                client: self.client(),
                get_txid: false,
                outputs: indexes
                    .iter()
                    .map(|index| GetOutputsOut {
                        amount: 0,
                        index: *index,
                    })
                    .collect(),
            };
            let outs: GetOutputsResponse = self
                .bin_call_limited(
                    "get_outs.bin",
                    monero_epee_bin_serde::to_bytes(&request).map_err(|_| {
                        RpcError::InternalError("epee serialization failed for get_outs request")
                    })?,
                    MIB + indexes.len() * 256,
                )
                .await?;

            require_ok_status(&outs.status)?;
            if outs.outs.len() != indexes.len() {
                return Err(RpcError::InvalidNode);
            }

            result.extend(
                outs.outs
                    .iter()
                    .map(|out| {
                        let point = |bytes: &[u8]| {
                            let compressed = CompressedEdwardsY(
                                bytes.try_into().map_err(|_| RpcError::InvalidNode)?,
                            );
                            compressed
                                .decompress()
                                .ok_or_else(|| RpcError::InvalidPoint(hex::encode(bytes)))
                        };
                        Ok((out.unlocked
                            && usize::try_from(out.height).map_or(false, |h| h <= height))
                        .then(|| Ok([point(&out.key)?, point(&out.mask)?]))
                        .transpose()?)
                    })
                    .collect::<Result<Vec<_>, RpcError>>()?,
            );
        }
        Ok(result)
    }

    async fn wallet2_automatic_uses_normal(
        &self,
        low_fee: u64,
        wallet_height: Option<u64>,
    ) -> Result<bool, RpcError> {
        #[derive(Deserialize, Debug)]
        struct Header {
            #[serde(default)]
            block_weight: u64,
        }
        #[derive(Deserialize, Debug)]
        struct HeadersResponse {
            headers: Vec<Header>,
            #[serde(default)]
            status: String,
        }

        if low_fee == 0 {
            return Err(RpcError::ZeroFee);
        }

        let request = epee_json(&json_rpc_request(
            "get_txpool_backlog",
            self.with_client("get_txpool_backlog", None),
        ));
        let limit = response_limit("json_rpc", Some("get_txpool_backlog"));
        let backlog = parse_txpool_backlog(&self.send("json_rpc", request, limit).await?)?;
        let info = self.node_info().await?;
        let full_reward_zone = info.block_weight_limit / 2;
        if full_reward_zone == 0 {
            return Err(RpcError::InvalidNode);
        }

        // wallet2's arithmetic: rates compared as doubles, weights summed in a
        // wrapping u64.
        let backlog_weight = backlog
            .iter()
            .filter(|entry| {
                entry.weight != 0 && entry.fee as f64 / entry.weight as f64 >= low_fee as f64
            })
            .fold(0u64, |sum, entry| sum.wrapping_add(entry.weight));
        if backlog_weight / full_reward_zone > 0 {
            return Ok(true);
        }

        const BLOCK_COUNT: u64 = 10;
        // wallet2 asks for the blocks below its own height, and the callers take
        // a refusal as "not congested".
        let header_height = wallet_height.unwrap_or(info.height);
        if header_height < BLOCK_COUNT {
            return Err(RpcError::InvalidNode);
        }
        let headers: HeadersResponse = self
            .json_rpc_call(
                "getblockheadersrange",
                Some(json!({
                    "start_height": header_height - BLOCK_COUNT,
                    "end_height": header_height - 1,
                })),
            )
            .await?;
        require_ok_status(&headers.status)?;
        if headers.headers.len() != usize::try_from(BLOCK_COUNT).unwrap() {
            return Err(RpcError::InvalidNode);
        }
        let weight = headers
            .headers
            .iter()
            .fold(0u64, |sum, header| sum.wrapping_add(header.block_weight));
        let capacity = BLOCK_COUNT.wrapping_mul(full_reward_zone);
        let percent_full = 100u64
            .wrapping_mul(weight)
            .checked_div(capacity)
            .ok_or(RpcError::InvalidNode)?;
        Ok(percent_full > 80)
    }

    /// A wallet2 fee tier, unchecked; prefer `get_fee_checked_for_priority`.
    pub async fn get_fee_for_priority(&self, priority: FeePriority) -> Result<Fee, RpcError> {
        self.get_fee_for_priority_at_height(priority, None).await
    }

    /// A wallet2 fee tier, with `Automatic` judging the ten blocks below
    /// `wallet_height`, as wallet2 does; `None` uses the node's height.
    pub async fn get_fee_for_priority_at_height(
        &self,
        priority: FeePriority,
        wallet_height: Option<u64>,
    ) -> Result<Fee, RpcError> {
        Ok(self
            .fee_tier_for_priority_at_height(priority, wallet_height)
            .await?
            .1)
    }

    async fn fetch_fee_estimate(&self) -> Result<FeeEstimate, RpcError> {
        #[allow(dead_code)]
        #[derive(Deserialize, Debug)]
        struct FeeResponse {
            #[serde(default)]
            fee: Option<u64>,
            #[serde(default)]
            fees: Vec<u64>,
            #[serde(default)]
            quantization_mask: Option<u64>,
            #[serde(default)]
            status: String,
        }

        let res: FeeResponse = self
            .json_rpc_call("get_fee_estimate", Some(json!({ "grace_blocks": 10 })))
            .await?;
        require_ok_status(&res.status)?;
        let mask = res
            .quantization_mask
            .ok_or(RpcError::InvalidFeeEstimate("no quantization mask"))?;
        let tiers = if res.fees.is_empty() {
            vec![res.fee.ok_or(RpcError::InvalidFeeEstimate("no fee rate"))?]
        } else {
            res.fees
        };
        Ok(FeeEstimate { mask, tiers })
    }

    async fn node_info(&self) -> Result<NodeInfo, RpcError> {
        #[derive(Deserialize, Debug)]
        struct InfoResponse {
            height: u64,
            #[serde(default)]
            block_weight_limit: u64,
            #[serde(default)]
            block_size_limit: u64,
            #[serde(default)]
            status: String,
        }

        let recent = self.1.state().info(current_time_secs());
        if let Some(info) = recent {
            return Ok(info);
        }
        let info: InfoResponse = self.json_rpc_call("get_info", Some(json!({}))).await?;
        require_ok_status(&info.status)?;
        let info = NodeInfo {
            height: info.height,
            block_weight_limit: if info.block_weight_limit == 0 {
                info.block_size_limit
            } else {
                info.block_weight_limit
            },
        };
        if let Some(now) = current_time_secs() {
            self.1.state().info = Some((now, info));
        }
        Ok(info)
    }

    async fn fee_estimate(&self, wallet_height: Option<u64>) -> Result<FeeEstimate, RpcError> {
        let now = current_time_secs();
        let stale = wallet_height
            .is_some_and(|height| !self.1.state().wallet_height_is_current(height, now));
        let height = if stale {
            Some(self.node_info().await?.height)
        } else {
            wallet_height
        };
        let known = self.1.state().fee_estimate(height, now);
        if let Some(estimate) = known {
            return Ok(estimate);
        }
        let estimate = self.fetch_fee_estimate().await?;
        self.1.state().fee_estimate = Some((height, now, estimate.clone()));
        Ok(estimate)
    }

    /// wallet2's `adjust_priority`: settle `Automatic` as `Low` or `Normal`. Do it
    /// once per transaction and pass the result to every call that builds it.
    pub async fn adjust_priority(
        &self,
        priority: FeePriority,
        wallet_height: Option<u64>,
    ) -> Result<FeePriority, RpcError> {
        if priority != FeePriority::Automatic {
            return Ok(priority);
        }
        let low_fee = self.fee_estimate(wallet_height).await?.tiers[0];
        let congested = self
            .wallet2_automatic_uses_normal(low_fee, wallet_height)
            .await
            .unwrap_or(false);
        Ok(if congested {
            FeePriority::Normal
        } else {
            FeePriority::Low
        })
    }

    async fn fee_tier_for_priority_at_height(
        &self,
        priority: FeePriority,
        wallet_height: Option<u64>,
    ) -> Result<(usize, Fee), RpcError> {
        let FeeEstimate { mask, tiers } = self.fee_estimate(wallet_height).await?;
        let tier = match priority.tier() {
            Some(tier) => tier,
            None => usize::from(
                self.wallet2_automatic_uses_normal(tiers[0], wallet_height)
                    .await
                    .unwrap_or(false),
            ),
        };
        let per_weight = *tiers.get(tier).ok_or(RpcError::InvalidFeeEstimate(
            "no rate for the requested priority",
        ))?;
        Ok((tier, Fee { per_weight, mask }))
    }

    /// Get wallet2's congestion-sensitive automatic fee tier.
    pub async fn get_fee(&self) -> Result<Fee, RpcError> {
        self.get_fee_for_priority(FeePriority::Automatic).await
    }

    /// The automatic fee, refused if the rate is zero or above `max_fee_per_byte`
    /// or its tier's bound, or the mask is zero or above its bound.
    pub async fn get_fee_checked(&self, max_fee_per_byte: u64) -> Result<Fee, RpcError> {
        self.get_fee_checked_for_priority(FeePriority::Automatic, max_fee_per_byte)
            .await
    }

    /// Get a selected wallet2 fee tier with a sanity check.
    pub async fn get_fee_checked_for_priority(
        &self,
        priority: FeePriority,
        max_fee_per_byte: u64,
    ) -> Result<Fee, RpcError> {
        self.get_fee_checked_for_priority_at_height(priority, None, max_fee_per_byte)
            .await
    }

    /// `get_fee_checked_for_priority` at a wallet height (see
    /// `get_fee_for_priority_at_height`).
    pub async fn get_fee_checked_for_priority_at_height(
        &self,
        priority: FeePriority,
        wallet_height: Option<u64>,
        max_fee_per_byte: u64,
    ) -> Result<Fee, RpcError> {
        let (tier, fee) = self
            .fee_tier_for_priority_at_height(priority, wallet_height)
            .await?;

        if fee.per_weight == 0 {
            return Err(RpcError::ZeroFee);
        }

        if fee.mask == 0 {
            return Err(RpcError::ZeroMask);
        }

        let tier_limit = MAX_DAEMON_FEE_PER_BYTE[tier];
        if fee.per_weight > tier_limit {
            return Err(RpcError::FeeExceedsLimit {
                per_weight: fee.per_weight,
                limit: tier_limit,
            });
        }

        if fee.per_weight > max_fee_per_byte {
            return Err(RpcError::FeeExceedsLimit {
                per_weight: fee.per_weight,
                limit: max_fee_per_byte,
            });
        }

        // The node also chooses the mask; unbounded, it defeats the rate limit.
        if fee.mask > MAX_DAEMON_FEE_QUANTIZATION_MASK {
            return Err(RpcError::MaskExceedsLimit {
                mask: fee.mask,
                limit: MAX_DAEMON_FEE_QUANTIZATION_MASK,
            });
        }

        Ok(fee)
    }

    /// Hand a transaction to the daemon, without the ring database check: a
    /// wallet relays through `native::broadcast_transaction*` or
    /// `native::broadcast_raw_transaction*`.
    pub async fn publish_transaction(&self, tx: &Transaction) -> Result<(), RpcError> {
        #[allow(dead_code)]
        #[derive(Deserialize, Debug)]
        struct SendRawResponse {
            status: String,
            double_spend: bool,
            fee_too_low: bool,
            invalid_input: bool,
            invalid_output: bool,
            low_mixin: bool,
            not_relayed: bool,
            overspend: bool,
            too_big: bool,
            too_few_outputs: bool,
            reason: String,
        }

        // As wallet2 relays: /sendrawtransaction with the transaction and
        // `client` only.
        let res: SendRawResponse = self
            .rpc_call(
                "sendrawtransaction",
                Some(json!({ "tx_as_hex": hex::encode(tx.serialize()) })),
            )
            .await?;

        if res.status != "OK" {
            Err(RpcError::InvalidTransaction(tx.hash()))?;
        }

        Ok(())
    }

    pub async fn generate_blocks(&self, address: &str, block_count: usize) -> Result<(), RpcError> {
        self.rpc_call::<_, EmptyResponse>(
            "json_rpc",
            Some(json!({
              "method": "generateblocks",
              "params": {
                "wallet_address": address,
                "amount_of_blocks": block_count
              },
            })),
        )
        .await?;

        Ok(())
    }

    /// The ids of every pool transaction, asked for as wallet2 asks.
    pub async fn get_pool_hashes(&self) -> Result<Vec<[u8; 32]>, RpcError> {
        let route = "get_transaction_pool_hashes.bin";
        let body = epee_json(&json!({ "client": self.client() }));
        let reply = self.send(route, body, response_limit(route, None)).await?;
        let status = epee_json_string(&reply, "status").ok_or(RpcError::InvalidNode)?;
        require_ok_status(std::str::from_utf8(&status).map_err(|_| RpcError::InvalidNode)?)?;
        let present = reply
            .windows(b"\"tx_hashes\": \"".len())
            .any(|window| window == b"\"tx_hashes\": \"");
        let blob = match epee_json_string(&reply, "tx_hashes") {
            Some(blob) => blob,
            None if !present => Vec::new(),
            None => return Err(RpcError::InvalidNode),
        };
        if blob.len() % 32 != 0 {
            return Err(RpcError::InvalidNode);
        }
        let hashes: Vec<[u8; 32]> = blob
            .chunks(32)
            .map(|hash| <[u8; 32]>::try_from(hash).unwrap())
            .collect();
        let mut seen = std::collections::HashSet::new();
        if !hashes.iter().all(|hash| seen.insert(*hash)) {
            return Err(RpcError::InvalidNode);
        }
        Ok(hashes)
    }

    /// Pool transactions by id, pruned, asked for as wallet2 asks; those no
    /// longer in the pool are left out.
    pub async fn get_pool_transactions(
        &self,
        hashes: &[[u8; 32]],
    ) -> Result<Vec<PoolTx>, RpcError> {
        #[derive(Deserialize, Debug)]
        struct Entry {
            tx_hash: String,
            #[serde(default)]
            pruned_as_hex: String,
            #[serde(default)]
            prunable_hash: String,
            #[serde(default)]
            in_pool: bool,
            #[serde(default)]
            double_spend_seen: bool,
        }
        #[derive(Deserialize, Debug)]
        struct Response {
            status: String,
            #[serde(default)]
            txs: Vec<Entry>,
        }

        let mut result = Vec::with_capacity(hashes.len());
        for slice in hashes.chunks(100) {
            let res: Response = self
                .rpc_call_limited(
                    "gettransactions",
                    Some(json!({
                        "txs_hashes": slice.iter().map(hex::encode).collect::<Vec<_>>(),
                        "decode_as_json": false,
                        "prune": true,
                    })),
                    transactions_limit(slice.len()),
                )
                .await?;
            require_ok_status(&res.status)?;
            if res.txs.len() != slice.len() {
                return Err(RpcError::InvalidNode);
            }
            for entry in res.txs {
                let tx_hash = hash_hex(&entry.tx_hash)?;
                if !slice.contains(&tx_hash) {
                    return Err(RpcError::InvalidNode);
                }
                if !entry.in_pool {
                    continue;
                }
                let blob = rpc_hex(&entry.pruned_as_hex)?;
                let prunable_hash = if entry.prunable_hash.is_empty() {
                    [0u8; 32]
                } else {
                    hash_hex(&entry.prunable_hash)?
                };
                let mut with_hash = blob.clone();
                with_hash.extend_from_slice(&prunable_hash);
                match crate::scanner::parse_pruned_tx_blob(&with_hash) {
                    Some((Some(hash), _)) if hash != tx_hash => return Err(RpcError::InvalidNode),
                    Some(_) => {}
                    None => return Err(RpcError::InvalidTransaction(tx_hash)),
                }
                result.push(PoolTx {
                    tx_hash,
                    blob,
                    double_spend_seen: entry.double_spend_seen,
                });
            }
        }
        Ok(result)
    }

    /// Every pool transaction, unpruned, and the key images they spend.
    pub async fn get_transaction_pool(
        &self,
    ) -> Result<(Vec<Transaction>, Vec<[u8; 32]>), RpcError> {
        let hashes = self.get_pool_hashes().await?;
        let mut transactions = Vec::with_capacity(hashes.len());
        for slice in hashes.chunks(100) {
            match self.get_transactions(slice).await {
                Ok(found) => transactions.extend(found),
                Err(RpcError::TransactionsNotFound(missing)) => {
                    let present: Vec<[u8; 32]> = slice
                        .iter()
                        .copied()
                        .filter(|hash| !missing.contains(hash))
                        .collect();
                    transactions.extend(self.get_transactions(&present).await?);
                }
                Err(error) => return Err(error),
            }
        }
        let spent_key_images = transactions
            .iter()
            .flat_map(|tx| tx.prefix().inputs.iter())
            .filter_map(|input| match input {
                Input::ToKey { key_image, .. } => Some(key_image.to_bytes()),
                Input::Gen(_) => None,
            })
            .collect();
        Ok((transactions, spent_key_images))
    }

    /// Fetch multiple blocks at once using the binary `/getblocks.bin` endpoint.
    ///
    /// Returns up to ~1000 blocks starting from `start_height`. The
    /// `block_ids` parameter should contain at least one known block hash for
    /// the daemon to find the fork point.
    pub async fn get_blocks_fast(
        &self,
        block_ids: &[[u8; 32]],
        start_height: u64,
        prune: bool,
    ) -> Result<GetBlocksFastResponse, RpcError> {
        Ok(self
            .get_blocks(block_ids, start_height, prune, None)
            .await?
            .0)
    }

    /// `get_blocks_fast` that also asks for the pool, as wallet2's refresh does.
    /// This call does not check pool transactions against their ids.
    pub async fn get_blocks_and_pool(
        &self,
        block_ids: &[[u8; 32]],
        start_height: u64,
        prune: bool,
        pool_info_since: u64,
    ) -> Result<(GetBlocksFastResponse, Option<PoolInfo>), RpcError> {
        self.get_blocks(block_ids, start_height, prune, Some(pool_info_since))
            .await
    }

    async fn get_blocks(
        &self,
        block_ids: &[[u8; 32]],
        start_height: u64,
        prune: bool,
        pool_info_since: Option<u64>,
    ) -> Result<(GetBlocksFastResponse, Option<PoolInfo>), RpcError> {
        // block_ids must be serialized as a contiguous blob
        // (KV_SERIALIZE_CONTAINER_POD_AS_BLOB)
        let block_ids_blob: Vec<u8> = block_ids.iter().flat_map(|h| h.iter().copied()).collect();

        // Fields in name order, optional ones left out unless set, as wallet2
        // writes them.
        #[derive(Serialize, Debug)]
        struct Request {
            block_ids: Vec<u8>,
            client: String,
            #[serde(skip_serializing_if = "is_zero")]
            pool_info_since: u64,
            prune: bool,
            #[serde(skip_serializing_if = "is_zero_u8")]
            requested_info: u8,
            start_height: u64,
        }
        fn is_zero(value: &u64) -> bool {
            *value == 0
        }
        fn is_zero_u8(value: &u8) -> bool {
            *value == 0
        }

        let req = Request {
            block_ids: block_ids_blob,
            client: self.client(),
            pool_info_since: pool_info_since.unwrap_or(0),
            prune,
            requested_info: u8::from(pool_info_since.is_some()),
            start_height,
        };
        let body = monero_epee_bin_serde::to_bytes(&req).map_err(|_| {
            RpcError::InternalError("epee serialization failed for getblocks.bin request")
        })?;
        let limit = match (pool_info_since.is_some(), prune) {
            (true, true) => MAX_PRUNED_POOL_REPLY_BYTES,
            (true, false) => MAX_POOL_REPLY_BYTES,
            (false, _) => response_limit("getblocks.bin", None),
        };
        let full: GetBlocksFastFullResponse =
            self.bin_call_limited("getblocks.bin", body, limit).await?;

        require_ok_status(&full.status)?;
        // A pruned reply to an unpruned request would skip the check that each
        // transaction is the one its block lists.
        if !prune && full.blocks.iter().any(|block| block.pruned) {
            return Err(RpcError::InvalidNode);
        }

        let pool = match (pool_info_since.is_some(), full.pool_info_extent) {
            (false, 0) => None,
            (false, _) => return Err(RpcError::InvalidNode),
            (true, 0) => None,
            (true, extent @ (1 | 2)) => {
                let incremental = extent == 1;
                let hashes = |blob: &[u8]| -> Result<Vec<[u8; 32]>, RpcError> {
                    if blob.len() % 32 != 0 {
                        return Err(RpcError::InvalidNode);
                    }
                    Ok(blob
                        .chunks(32)
                        .map(|hash| <[u8; 32]>::try_from(hash).unwrap())
                        .collect())
                };
                let added = full
                    .added_pool_txs
                    .into_iter()
                    .map(|tx| {
                        Ok(PoolTx {
                            tx_hash: <[u8; 32]>::try_from(tx.tx_hash.as_slice())
                                .map_err(|_| RpcError::InvalidNode)?,
                            blob: tx.tx_blob,
                            double_spend_seen: tx.double_spend_seen,
                        })
                    })
                    .collect::<Result<Vec<_>, RpcError>>()?;
                let remaining_added = hashes(&full.remaining_added_pool_txids)?;
                let removed = hashes(&full.removed_pool_txids)?;
                if !incremental && !removed.is_empty() {
                    return Err(RpcError::InvalidNode);
                }
                if incremental && pool_info_since == Some(0) {
                    return Err(RpcError::InvalidNode);
                }
                let mut seen = std::collections::HashSet::new();
                if !added
                    .iter()
                    .map(|tx| tx.tx_hash)
                    .chain(remaining_added.iter().copied())
                    .all(|hash| seen.insert(hash))
                {
                    return Err(RpcError::InvalidNode);
                }
                Some(PoolInfo {
                    incremental,
                    added,
                    remaining_added,
                    removed,
                    daemon_time: full.daemon_time,
                })
            }
            (true, _) => return Err(RpcError::InvalidNode),
        };

        Ok((
            GetBlocksFastResponse {
                status: full.status,
                blocks: full.blocks,
                start_height: full.start_height,
                current_height: full.current_height,
                output_indices: full.output_indices,
                untrusted: full.untrusted,
                credits: full.credits,
                top_hash: full.top_hash,
                daemon_time: full.daemon_time,
            },
            pool,
        ))
    }
}

/// Transaction ids per /get_transactions request: the most a restricted node
/// answers.
const TRANSACTIONS_PER_REQUEST: usize = 100;

fn transactions_limit(count: usize) -> usize {
    count
        .saturating_add(1)
        .saturating_mul(MIB)
        .min(MAX_RESPONSE_BYTES)
}

fn pruned_entry_error(blob: &[u8], prunable_hash: &str, hash: [u8; 32]) -> RpcError {
    let prunable_hash = if prunable_hash.is_empty() {
        [0u8; 32]
    } else {
        match hash_hex(prunable_hash) {
            Ok(prunable_hash) => prunable_hash,
            Err(error) => return error,
        }
    };
    let mut with_hash = blob.to_vec();
    with_hash.extend_from_slice(&prunable_hash);
    match crate::scanner::parse_pruned_tx_blob(&with_hash) {
        Some((Some(id), _)) if id != hash => RpcError::InvalidNode,
        Some(_) => RpcError::PrunedTransaction(hash),
        None => RpcError::InvalidTransaction(hash),
    }
}

/// Parse the additional timelock out of a transaction prefix: the version
/// varint followed by the timelock varint.
fn prefix_timelock(data: &[u8]) -> Result<Timelock, RpcError> {
    fn read_varint(data: &[u8], position: &mut usize) -> Result<u64, RpcError> {
        let mut bits = 0u32;
        let mut res = 0u64;
        loop {
            let byte = *data.get(*position).ok_or(RpcError::InvalidNode)?;
            *position += 1;
            res |= u64::from(byte & 0x7f)
                .checked_shl(bits)
                .ok_or(RpcError::InvalidNode)?;
            if byte & 0x80 == 0 {
                return Ok(res);
            }
            bits += 7;
            if bits >= 64 {
                return Err(RpcError::InvalidNode);
            }
        }
    }

    let mut position = 0;
    let _version = read_varint(data, &mut position)?;
    let raw = read_varint(data, &mut position)?;
    // Monero interprets timelocks under 500 million as block numbers
    const TIMELOCK_BLOCK_THRESHOLD: u64 = 500_000_000;
    Ok(if raw == 0 {
        Timelock::None
    } else if raw < TIMELOCK_BLOCK_THRESHOLD {
        Timelock::Block(usize::try_from(raw).map_err(|_| RpcError::InvalidNode)?)
    } else {
        Timelock::Time(raw)
    })
}

/// Deserialize a single `Vec<u8>` from an epee byte-blob field.
mod epee_byte_blob {
    use serde::de::{Deserializer, Visitor};
    use std::fmt;

    struct ByteBlobVisitor;

    impl<'de> Visitor<'de> for ByteBlobVisitor {
        type Value = Vec<u8>;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a byte blob")
        }

        fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
            Ok(v.to_vec())
        }

        fn visit_byte_buf<E: serde::de::Error>(self, v: Vec<u8>) -> Result<Self::Value, E> {
            Ok(v)
        }

        fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
            Ok(v.into_bytes())
        }

        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
            Ok(v.as_bytes().to_vec())
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_byte_buf(ByteBlobVisitor)
    }

    #[allow(dead_code)]
    pub fn serialize<S>(data: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_bytes(data)
    }
}

/// Deserialize a `Vec<Vec<u8>>` where each element is an epee byte-blob field.
mod vec_of_bytes {
    use serde::de::{Deserializer, SeqAccess, Visitor};
    use std::fmt;

    struct ByteBlob;

    impl<'de> serde::de::DeserializeSeed<'de> for ByteBlob {
        type Value = Vec<u8>;

        fn deserialize<D: Deserializer<'de>>(
            self,
            deserializer: D,
        ) -> Result<Self::Value, D::Error> {
            struct ByteBlobVisitor;

            impl<'de> Visitor<'de> for ByteBlobVisitor {
                type Value = Vec<u8>;

                fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                    f.write_str("a byte blob")
                }

                fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
                    Ok(v.to_vec())
                }

                fn visit_byte_buf<E: serde::de::Error>(self, v: Vec<u8>) -> Result<Self::Value, E> {
                    Ok(v)
                }

                fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
                    Ok(v.into_bytes())
                }

                fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                    Ok(v.as_bytes().to_vec())
                }

                fn visit_map<A: serde::de::MapAccess<'de>>(
                    self,
                    mut map: A,
                ) -> Result<Self::Value, A::Error> {
                    use serde::de::Error;

                    let mut blob = None;
                    let mut prunable_hash = None;
                    while let Some(key) = map.next_key::<String>()? {
                        match key.as_str() {
                            "blob" => blob = Some(map.next_value_seed(ByteBlob)?),
                            "prunable_hash" => prunable_hash = Some(map.next_value_seed(ByteBlob)?),
                            _ => {
                                map.next_value::<serde::de::IgnoredAny>()?;
                            }
                        }
                    }
                    let mut blob = blob.ok_or_else(|| A::Error::missing_field("blob"))?;
                    let prunable_hash =
                        prunable_hash.ok_or_else(|| A::Error::missing_field("prunable_hash"))?;
                    if prunable_hash.len() != 32 {
                        return Err(A::Error::custom("prunable_hash is not 32 bytes"));
                    }
                    blob.extend_from_slice(&prunable_hash);
                    Ok(blob)
                }
            }

            deserializer.deserialize_byte_buf(ByteBlobVisitor)
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<Vec<u8>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct VecOfBytesVisitor;

        impl<'de> Visitor<'de> for VecOfBytesVisitor {
            type Value = Vec<Vec<u8>>;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a sequence of byte blobs")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut result = Vec::new();
                while let Some(bytes) = seq.next_element_seed(ByteBlob)? {
                    result.push(bytes);
                }
                Ok(result)
            }
        }

        deserializer.deserialize_seq(VecOfBytesVisitor)
    }

    #[allow(dead_code)]
    pub fn serialize<S>(data: &[Vec<u8>], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(data.len()))?;
        for item in data {
            seq.serialize_element(&serde_bytes::Bytes::new(item))?;
        }
        seq.end()
    }
}

/// A single block entry returned by `/getblocks.bin`.
#[allow(dead_code)]
#[derive(Deserialize, Debug, Clone)]
pub struct BlockCompleteEntry {
    /// Raw block blob (header + miner tx + tx hashes).
    #[serde(with = "epee_byte_blob")]
    pub block: Vec<u8>,
    /// Raw transaction blobs: whole when `pruned` is false, otherwise each the
    /// pruned transaction followed by the 32-byte hash of its pruned part.
    #[serde(default, with = "vec_of_bytes")]
    pub txs: Vec<Vec<u8>>,
    /// Whether the transaction data is pruned.
    #[serde(default)]
    pub pruned: bool,
    /// Block weight.
    #[serde(default)]
    pub block_weight: u64,
}

/// The most block ids monerod sends in one `/gethashes.bin` reply.
pub const MAX_BLOCK_IDS: usize = 10_000;

/// A `/gethashes.bin` answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockIds {
    pub start_height: u64,
    pub current_height: u64,
    pub ids: Vec<[u8; 32]>,
}

/// Output indices for a single transaction.
#[derive(Deserialize, Debug, Clone)]
pub struct TxOutputIndices {
    #[serde(default)]
    pub indices: Vec<u64>,
}

/// Output indices for all transactions in a block.
#[derive(Deserialize, Debug, Clone)]
pub struct BlockOutputIndices {
    #[serde(default)]
    pub indices: Vec<TxOutputIndices>,
}

/// Response from the `/getblocks.bin` binary RPC endpoint.
#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct GetBlocksFastResponse {
    pub status: String,
    #[serde(default)]
    pub blocks: Vec<BlockCompleteEntry>,
    pub start_height: u64,
    pub current_height: u64,
    #[serde(default)]
    pub output_indices: Vec<BlockOutputIndices>,
    #[serde(default)]
    pub untrusted: bool,
    #[serde(default)]
    pub credits: u64,
    #[serde(default)]
    pub top_hash: String,
    #[serde(default)]
    pub daemon_time: u64,
}

/// One pool transaction a getblocks.bin reply carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolTx {
    /// The id the node gave; a pruned blob cannot be checked against it.
    pub tx_hash: [u8; 32],
    /// The transaction, pruned when pruned data was asked for.
    pub blob: Vec<u8>,
    pub double_spend_seen: bool,
}

/// The pool part of a getblocks.bin reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolInfo {
    /// Whether `added` and `removed` are changes since `pool_info_since`, or
    /// `added` is the whole pool.
    pub incremental: bool,
    /// Transactions new in the pool, or the whole pool.
    pub added: Vec<PoolTx>,
    /// Further new transactions a restricted node lists by id only.
    pub remaining_added: Vec<[u8; 32]>,
    /// Transactions that left the pool, in an incremental answer.
    pub removed: Vec<[u8; 32]>,
    /// The node's clock when it answered; the `pool_info_since` of the next
    /// request.
    pub daemon_time: u64,
}

#[derive(Deserialize, Debug)]
struct PoolTxEntry {
    #[serde(with = "epee_byte_blob")]
    tx_hash: Vec<u8>,
    #[serde(with = "epee_byte_blob")]
    tx_blob: Vec<u8>,
    #[serde(default)]
    double_spend_seen: bool,
}

#[derive(Deserialize, Debug)]
struct GetBlocksFastFullResponse {
    status: String,
    #[serde(default)]
    blocks: Vec<BlockCompleteEntry>,
    start_height: u64,
    current_height: u64,
    #[serde(default)]
    output_indices: Vec<BlockOutputIndices>,
    #[serde(default)]
    untrusted: bool,
    #[serde(default)]
    credits: u64,
    #[serde(default)]
    top_hash: String,
    #[serde(default)]
    daemon_time: u64,
    #[serde(default)]
    pool_info_extent: u8,
    #[serde(default)]
    added_pool_txs: Vec<PoolTxEntry>,
    #[serde(default, with = "epee_byte_blob")]
    remaining_added_pool_txids: Vec<u8>,
    #[serde(default, with = "epee_byte_blob")]
    removed_pool_txids: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn decoy_outputs_use_wallet2s_binary_rpc_without_txids() {
        use std::sync::{Arc, Mutex};

        #[derive(Clone, Debug)]
        struct Connection(Arc<Mutex<Vec<(String, Vec<u8>)>>>);
        #[async_trait]
        impl RpcConnection for Connection {
            async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
                self.0.lock().unwrap().push((route.to_string(), body));
                Err(RpcError::ConnectionError)
            }
        }

        let calls = Arc::new(Mutex::new(Vec::new()));
        let rpc = Rpc::new_with_connection(Connection(calls.clone()));
        assert!(rpc.get_unlocked_outputs(&[7, 11], 100).await.is_err());

        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "get_outs.bin");
        #[derive(Deserialize)]
        struct Request {
            outputs: Vec<GetOutputsOut>,
            get_txid: bool,
        }
        let request: Request = crate::safe_epee::from_bytes(&calls[0].1).unwrap();
        assert!(!request.get_txid);
        assert_eq!(request.outputs.len(), 2);
        assert_eq!(request.outputs[0].amount, 0);
        assert_eq!(request.outputs[0].index, 7);
        assert_eq!(request.outputs[1].index, 11);

        let client = client_member(&calls[0].1);
        let mut expected = hex::decode("011101010101020101").unwrap();
        expected.extend_from_slice(&[0x0c, 6]);
        expected.extend_from_slice(b"client\x0a\x41\x03");
        expected.extend_from_slice(client.as_bytes());
        expected.extend_from_slice(&[8]);
        expected.extend_from_slice(b"get_txid\x0b\x00");
        expected.extend_from_slice(&[7]);
        expected.extend_from_slice(b"outputs\x8c\x08");
        for index in [7u64, 11] {
            expected.extend_from_slice(&[0x08, 6]);
            expected.extend_from_slice(b"amount\x05");
            expected.extend_from_slice(&0u64.to_le_bytes());
            expected.extend_from_slice(&[5]);
            expected.extend_from_slice(b"index\x05");
            expected.extend_from_slice(&index.to_le_bytes());
        }
        assert_eq!(hex::encode(&calls[0].1), hex::encode(expected));
    }

    fn client_member(body: &[u8]) -> String {
        #[derive(Deserialize)]
        struct Client {
            client: String,
        }
        let client = crate::safe_epee::from_bytes::<Client, _>(body)
            .unwrap()
            .client;
        assert!(crate::rpc_client_id::tests::verify(&client).is_some());
        client
    }

    #[derive(Clone, Debug, Default)]
    struct Recorder(std::sync::Arc<std::sync::Mutex<Vec<(String, Vec<u8>)>>>);
    #[async_trait]
    impl RpcConnection for Recorder {
        async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
            self.0.lock().unwrap().push((route.to_string(), body));
            Err(RpcError::ConnectionError)
        }
    }

    #[derive(Clone, Debug, Default)]
    struct ForkingNode {
        version: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }
    #[async_trait]
    impl RpcConnection for ForkingNode {
        async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
            use std::sync::atomic::Ordering;
            assert_eq!(route, "json_rpc");
            let request: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(request["method"], "hard_fork_info");
            assert_eq!(request["params"]["version"], 15);
            self.calls.fetch_add(1, Ordering::SeqCst);
            let version = self.version.load(Ordering::SeqCst);
            Ok(serde_json::to_vec(&json!({
                "id": "0", "jsonrpc": "2.0",
                "result": {"version": version, "earliest_height": 100,
                           "enabled": version >= 15, "status": "OK"}
            }))
            .unwrap())
        }
    }

    #[tokio::test]
    async fn a_long_lived_connection_follows_a_fork() {
        use std::sync::atomic::Ordering;
        let node = ForkingNode::default();
        node.version.store(14, Ordering::SeqCst);
        let rpc = Rpc::new_with_connection(node.clone());
        let calls = || node.calls.load(Ordering::SeqCst);

        assert_eq!(
            rpc.get_rct_type_at_height(Some(99)).await.unwrap(),
            RctType::ClsagBulletproof
        );
        node.version.store(15, Ordering::SeqCst);
        assert_eq!(
            rpc.get_rct_type_at_height(Some(100)).await.unwrap(),
            RctType::ClsagBulletproofPlus
        );
        assert_eq!(calls(), 1);

        assert_eq!(rpc.get_rct_type().await.unwrap(), RctType::ClsagBulletproof);
        assert_eq!(calls(), 1);
        let asked = rpc.1.state().fork.unwrap().at.unwrap();
        rpc.1.state().fork.as_mut().unwrap().at = Some(asked - NODE_CACHE_SECONDS);
        assert_eq!(
            rpc.get_rct_type().await.unwrap(),
            RctType::ClsagBulletproofPlus
        );
        assert_eq!(calls(), 2);
        rpc.1.state().fork.as_mut().unwrap().at = Some(0);
        for height in [None, Some(1), Some(1_000)] {
            assert_eq!(
                rpc.get_rct_type_at_height(height).await.unwrap(),
                RctType::ClsagBulletproofPlus
            );
        }
        assert_eq!(calls(), 2);
    }

    #[test]
    fn a_node_that_knows_no_fork_block_is_judged_by_its_version() {
        let fork = ForkInfo {
            version: 14,
            plus_from: u64::MAX,
            at: Some(1_000),
        };
        assert_eq!(fork.version_at(Some(u64::MAX - 1)), 14);
        assert!(fork.decides(Some(5), Some(1_029)));
        assert!(!fork.decides(Some(5), Some(1_030)));
        assert!(!fork.decides(None, None));
    }

    fn signer_of(body: &[u8]) -> String {
        let request: Value = serde_json::from_slice(body).unwrap();
        let client = request["params"]["client"].as_str().unwrap();
        assert!(crate::rpc_client_id::tests::verify(client).is_some());
        client[..64].to_string()
    }

    #[tokio::test]
    async fn every_request_of_a_wallet_is_signed_with_its_key() {
        let recorder = Recorder::default();
        let rpc = Rpc::new_with_connection(recorder.clone());
        let first = crate::wallet_state::WalletState::new();
        let second = crate::wallet_state::WalletState::new();
        assert_ne!(first.rpc_client_key(), second.rpc_client_key());
        for _ in 0..3 {
            for state in [&first, &second] {
                let rpc = rpc.for_wallet(state);
                assert!(rpc.fetch_fee_estimate().await.is_err());
                assert!(rpc.get_rct_type().await.is_err());
            }
        }
        assert!(rpc.fetch_fee_estimate().await.is_err());

        let calls = recorder.0.lock().unwrap();
        assert_eq!(calls.len(), 13);
        let signers: Vec<String> = calls.iter().map(|(_, body)| signer_of(body)).collect();
        let key = |state: &crate::wallet_state::WalletState| {
            hex::encode(state.rpc_client_key().public_key())
        };
        for (index, signer) in signers[..12].iter().enumerate() {
            let state = if index % 4 < 2 { &first } else { &second };
            assert_eq!(*signer, key(state), "request {index}");
        }
        assert_eq!(signers[12], hex::encode(rpc.client_key().public_key()));
        assert_ne!(signers[12], key(&first));
        assert_ne!(signers[12], key(&second));

        let mut restored = crate::wallet_state::WalletState::new();
        restored.restore_rpc_client_key(
            RpcClientKey::from_secret_bytes(&first.rpc_client_key().to_secret_bytes()).unwrap(),
        );
        assert_eq!(
            rpc.for_wallet(&restored).client_key(),
            first.rpc_client_key()
        );
    }

    #[tokio::test]
    async fn a_wallets_answers_are_not_another_wallets() {
        #[derive(Clone, Debug, Default)]
        struct Node(std::sync::Arc<std::sync::Mutex<Vec<String>>>);
        #[async_trait]
        impl RpcConnection for Node {
            async fn post(&self, _route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
                let request: Value = serde_json::from_slice(&body).unwrap();
                self.0
                    .lock()
                    .unwrap()
                    .push(request["method"].as_str().unwrap().to_string());
                Ok(serde_json::to_vec(&json!({
                    "id": "0", "jsonrpc": "2.0",
                    "result": {"block_header": {"hash": "11".repeat(32)}, "status": "OK"}
                }))
                .unwrap())
            }
        }
        let node = Node::default();
        let rpc = Rpc::new_with_connection(node.clone());
        let first = crate::wallet_state::WalletState::new();
        let second = crate::wallet_state::WalletState::new();
        for _ in 0..2 {
            rpc.for_wallet(&first).genesis_hash().await.unwrap();
        }
        rpc.for_wallet(&second).genesis_hash().await.unwrap();
        rpc.for_wallet(&second).genesis_hash().await.unwrap();
        assert_eq!(node.0.lock().unwrap().len(), 2);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn url_functions_keep_one_key_per_daemon() {
        let a = client_key_for_url("http://127.0.0.1:1");
        assert_eq!(client_key_for_url("http://127.0.0.1:1"), a);
        assert_ne!(client_key_for_url("http://127.0.0.1:2"), a);
        let rpc = url_rpc("http://127.0.0.1:1").unwrap();
        assert_eq!(rpc.client_key(), &a);
        assert_eq!(url_rpc("http://127.0.0.1:1").unwrap().client_key(), &a);
    }

    #[tokio::test]
    async fn json_requests_are_written_like_wallet2s() {
        let recorder = Recorder::default();
        let rpc = Rpc::new_with_connection(recorder.clone());
        assert!(rpc.fetch_fee_estimate().await.is_err());
        assert!(rpc
            .json_rpc_call::<Value>(
                "getblockheadersrange",
                Some(json!({ "start_height": 253, "end_height": 262 })),
            )
            .await
            .is_err());
        assert!(rpc.get_height().await.is_err());

        let calls = recorder.0.lock().unwrap();
        let bodies: Vec<_> = calls
            .iter()
            .map(|(route, body)| (route.as_str(), String::from_utf8(body.clone()).unwrap()))
            .collect();
        let clients: Vec<String> = bodies[..2]
            .iter()
            .map(|(_, body)| {
                let request: Value = serde_json::from_str(body).unwrap();
                let client = request["params"]["client"].as_str().unwrap().to_string();
                assert!(crate::rpc_client_id::tests::verify(&client).is_some());
                client
            })
            .collect();
        assert_eq!(clients[0][..64], clients[1][..64]);
        assert_eq!(
            bodies,
            vec![
                (
                    "json_rpc",
                    format!(
                        "{{\r\n  \"id\": \"0\",\r\n  \"jsonrpc\": \"2.0\",\r\n  \"method\": \
                         \"get_fee_estimate\",\r\n  \"params\": {{\r\n    \"client\": \"{}\",\r\n    \
                         \"grace_blocks\": 10\r\n  }}\r\n}}",
                        clients[0]
                    )
                ),
                (
                    "json_rpc",
                    format!(
                        "{{\r\n  \"id\": \"0\",\r\n  \"jsonrpc\": \"2.0\",\r\n  \"method\": \
                         \"getblockheadersrange\",\r\n  \"params\": {{\r\n    \"client\": \
                         \"{}\",\r\n    \"end_height\": 262,\r\n    \"start_height\": 253\r\n  \
                         }}\r\n}}",
                        clients[1]
                    )
                ),
                ("get_height", "{\r\n}".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn pool_ids_are_read_from_monerods_blob_string() {
        #[derive(Clone, Debug)]
        struct Node(Vec<u8>);
        #[async_trait]
        impl RpcConnection for Node {
            async fn post(&self, route: &str, _: Vec<u8>) -> Result<Vec<u8>, RpcError> {
                assert_eq!(route, "get_transaction_pool_hashes.bin");
                Ok(self.0.clone())
            }
        }
        let captured = hex::decode(concat!(
            "7b0d0a20202263726564697473223a20302c0d0a202022737461747573223a2022",
            "4f4b222c0d0a202022746f705f68617368223a2022222c0d0a20202274785f6861",
            "73686573223a2022355c762d0100e9d458fa577ac88d9bb64bbcf8baca45cff559",
            "7189e618d70f9af1222c0d0a202022756e74727573746564223a2066616c73650d",
            "0a7d",
        ))
        .unwrap();
        let ids = Rpc::new_with_connection(Node(captured.clone()))
            .get_pool_hashes()
            .await
            .unwrap();
        assert_eq!(
            ids,
            vec![
                hash_hex("350b2d0100e9d458fa577ac88d9bb64bbcf8baca45cff5597189e618d70f9af1")
                    .unwrap()
            ]
        );
        let empty = b"{\r\n  \"credits\": 0,\r\n  \"status\": \"OK\"\r\n}".to_vec();
        assert_eq!(
            Rpc::new_with_connection(Node(empty))
                .get_pool_hashes()
                .await,
            Ok(vec![])
        );
        let unterminated = captured[..captured.len() - 40].to_vec();
        assert!(Rpc::new_with_connection(Node(unterminated))
            .get_pool_hashes()
            .await
            .is_err());
        let partial = b"{\r\n  \"status\": \"OK\",\r\n  \"tx_hashes\": \"abc\"\r\n}".to_vec();
        assert!(Rpc::new_with_connection(Node(partial))
            .get_pool_hashes()
            .await
            .is_err());
        let failed = b"{\r\n  \"status\": \"Failed\"\r\n}".to_vec();
        assert!(Rpc::new_with_connection(Node(failed))
            .get_pool_hashes()
            .await
            .is_err());
    }

    #[tokio::test]
    async fn a_pruned_pool_request_is_held_to_the_pruned_pool_limit() {
        #[derive(Clone, Debug, Default)]
        struct Limits(std::sync::Arc<std::sync::Mutex<Vec<usize>>>);
        #[async_trait]
        impl RpcConnection for Limits {
            async fn post(&self, _: &str, _: Vec<u8>) -> Result<Vec<u8>, RpcError> {
                unreachable!("Rpc calls post_limited")
            }
            async fn post_limited(
                &self,
                _: &str,
                _: Vec<u8>,
                limit: usize,
            ) -> Result<Vec<u8>, RpcError> {
                self.0.lock().unwrap().push(limit);
                Err(RpcError::ConnectionError)
            }
        }
        let limits = Limits::default();
        let rpc = Rpc::new_with_connection(limits.clone());
        let ids = [[0u8; 32]];
        assert!(rpc.get_blocks_and_pool(&ids, 0, true, 0).await.is_err());
        assert!(rpc.get_blocks_and_pool(&ids, 0, false, 0).await.is_err());
        assert!(rpc.get_blocks_fast(&ids, 0, true).await.is_err());
        assert_eq!(
            *limits.0.lock().unwrap(),
            vec![
                MAX_PRUNED_POOL_REPLY_BYTES,
                MAX_POOL_REPLY_BYTES,
                MAX_RESPONSE_BYTES
            ]
        );
        assert!(MAX_RESPONSE_BYTES < MAX_PRUNED_POOL_REPLY_BYTES);
        assert!(MAX_PRUNED_POOL_REPLY_BYTES < MAX_POOL_REPLY_BYTES);
    }

    #[test]
    fn epee_json_writes_arrays_empty_sections_and_escapes_like_epee() {
        let value = json!({
            "txs_hashes": ["ab", "cd"],
            "empty": {},
            "list": [{ "a": 1 }, { "b": true }],
            "text": "a/b\"c\\d\ne\u{b}",
        });
        assert_eq!(
            String::from_utf8(epee_json(&value)).unwrap(),
            "{\r\n  \"empty\": {\r\n  },\r\n  \"list\": [{\r\n    \"a\": 1\r\n  },{\r\n    \
             \"b\": true\r\n  }],\r\n  \"text\": \"a\\/b\\\"c\\\\d\\ne\\v\",\r\n  \
             \"txs_hashes\": [\"ab\",\"cd\"]\r\n}"
        );
    }

    #[tokio::test]
    async fn decoy_outputs_use_wallet2s_thousand_output_chunks() {
        use std::sync::{Arc, Mutex};

        #[derive(Clone, Debug)]
        struct Connection(Arc<Mutex<Vec<usize>>>);
        #[async_trait]
        impl RpcConnection for Connection {
            async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
                assert_eq!(route, "get_outs.bin");
                let request: GetOutputsRequest = crate::safe_epee::from_bytes(&body).unwrap();
                self.0.lock().unwrap().push(request.outputs.len());
                let point = curve25519_dalek::constants::ED25519_BASEPOINT_POINT
                    .compress()
                    .to_bytes()
                    .to_vec();
                monero_epee_bin_serde::to_bytes(&GetOutputsResponse {
                    outs: request
                        .outputs
                        .iter()
                        .map(|_| GetOutputsKey {
                            key: point.clone(),
                            mask: point.clone(),
                            unlocked: true,
                            height: 1,
                            txid: vec![],
                        })
                        .collect(),
                    status: "OK".to_string(),
                })
                .map_err(|_| RpcError::InternalError("test response serialization failed"))
            }
        }

        let chunks = Arc::new(Mutex::new(Vec::new()));
        let rpc = Rpc::new_with_connection(Connection(chunks.clone()));
        let indexes = (0..1001).collect::<Vec<_>>();
        assert_eq!(
            rpc.get_unlocked_outputs(&indexes, 100).await.unwrap().len(),
            indexes.len()
        );
        assert_eq!(*chunks.lock().unwrap(), vec![1000, 1]);
    }

    #[tokio::test]
    async fn decoy_distribution_uses_wallet2s_compressed_binary_rpc() {
        use std::sync::{Arc, Mutex};

        #[derive(Clone, Debug)]
        struct Connection(Arc<Mutex<Vec<(String, Vec<u8>)>>>);
        #[async_trait]
        impl RpcConnection for Connection {
            async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
                self.0.lock().unwrap().push((route.to_string(), body));
                Err(RpcError::ConnectionError)
            }
        }

        let calls = Arc::new(Mutex::new(Vec::new()));
        let rpc = Rpc::new_with_connection(Connection(calls.clone()));
        assert!(rpc.get_output_distribution(0, 100).await.is_err());

        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "get_output_distribution.bin");
        #[derive(Deserialize)]
        struct Request {
            amounts: Vec<u64>,
            compress: bool,
        }
        let request: Request = crate::safe_epee::from_bytes(&calls[0].1).unwrap();
        assert_eq!(request.amounts, vec![0]);
        assert!(request.compress);

        let wallet2 = hex::decode(concat!(
            "0111010101010201010c07616d6f756e747385040000000000000000",
            "06636c69656e740a4103",
            "3064386132353535633365333264393930663063306537353330353362353534",
            "6234643666623166663930316463316235643166336639666435666465633837",
            "3030303636303939633564633937373731333833363562396438306565353164",
            "3039653361623064663932326434383330363731666236303964396135383966",
            "6137383730653530613763643862306535663833316538373863396361363934",
            "3637633533356461343936656535666461393632336131613837343532623139",
            "3862386266303538636630346239303108636f6d70726573730b01",
        ))
        .unwrap();
        let client = client_member(&calls[0].1);
        let mut expected = wallet2[..38].to_vec();
        expected.extend_from_slice(client.as_bytes());
        expected.extend_from_slice(&wallet2[38 + 208..]);
        assert_eq!(hex::encode(&calls[0].1), hex::encode(expected));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn binary_rpc_rejects_truncation_and_recursive_unknown_fields() {
        #[derive(Clone, Debug)]
        struct Connection;
        #[async_trait]
        impl RpcConnection for Connection {
            async fn post(&self, _: &str, _: Vec<u8>) -> Result<Vec<u8>, RpcError> {
                unreachable!("response parsing requires no network")
            }
        }
        let rpc = Rpc::new_with_connection(Connection);
        // {x: {y: 42u64}} with both fields unknown to the target response.
        let valid = hex::decode("0111010101010201010401780c040179052a00000000000000").unwrap();
        for end in 0..valid.len() {
            assert!(rpc
                .call_tail::<EmptyResponse>("getblocks.bin", valid[..end].to_vec())
                .await
                .is_err());
        }
        rpc.call_tail::<EmptyResponse>("getblocks.bin", valid)
            .await
            .unwrap();
        let mut deep = b"\x01\x11\x01\x01\x01\x01\x02\x01\x01".to_vec();
        for _ in 0..1000 {
            deep.extend_from_slice(&[4, 1, b'x', 12]);
        }
        deep.push(0);
        assert!(rpc
            .call_tail::<EmptyResponse>("getblocks.bin", deep)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn replies_larger_than_the_call_allows_are_refused() {
        #[derive(Clone, Debug)]
        struct Connection(usize);
        #[async_trait]
        impl RpcConnection for Connection {
            async fn post(&self, _: &str, _: Vec<u8>) -> Result<Vec<u8>, RpcError> {
                let mut reply = b"{\"height\":7".to_vec();
                reply.resize(self.0 - 1, b' ');
                reply.push(b'}');
                Ok(reply)
            }
        }

        assert_eq!(response_limit("get_height", None), MIB);
        let rpc = Rpc::new_with_connection(Connection(MIB));
        assert_eq!(rpc.get_height().await.unwrap(), 7);
        let rpc = Rpc::new_with_connection(Connection(MIB + 1));
        assert_eq!(
            rpc.get_height().await,
            Err(RpcError::ResponseTooLarge { limit: MIB })
        );

        assert_eq!(response_limit("getblocks.bin", None), MAX_RESPONSE_BYTES);
        assert_eq!(response_limit("json_rpc", Some("get_fee_estimate")), MIB);
        assert!(response_limit("json_rpc", Some("get_txpool_backlog")) > MIB);
        assert_eq!(transactions_limit(3), 4 * MIB);
        assert_eq!(transactions_limit(usize::MAX), MAX_RESPONSE_BYTES);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn serve_and_record_headers(
        replies: Vec<&'static [u8]>,
    ) -> (u16, std::thread::JoinHandle<Vec<(String, Vec<String>)>>) {
        use std::io::{BufRead, BufReader, Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut writer = stream;
            let mut seen = Vec::new();
            for reply in replies {
                let mut request_line = String::new();
                reader.read_line(&mut request_line).unwrap();
                let mut headers = Vec::new();
                let mut length = 0usize;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    let line = line.trim_end_matches("\r\n").to_string();
                    if line.is_empty() {
                        break;
                    }
                    if let Some(value) = line.strip_prefix("Content-Length: ") {
                        length = value.parse().unwrap();
                    }
                    headers.push(line);
                }
                let mut body = vec![0u8; length];
                reader.read_exact(&mut body).unwrap();
                seen.push((request_line.trim_end().to_string(), headers));
                write!(
                    writer,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                    reply.len()
                )
                .unwrap();
                writer.write_all(reply).unwrap();
            }
            seen
        });
        (port, handle)
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn requests_carry_wallet2s_headers_in_wallet2s_order() {
        let (port, server) = serve_and_record_headers(vec![b"{\"height\": 7}", b"x"]);
        let rpc = HttpRpc::new(format!("http://127.0.0.1:{port}")).unwrap();
        assert_eq!(rpc.get_height().await.unwrap(), 7);
        assert!(rpc
            .bin_call::<EmptyResponse>("get_o_indexes.bin", vec![1, 2, 3])
            .await
            .is_err());
        assert_eq!(
            server.join().unwrap(),
            vec![
                (
                    "POST /get_height HTTP/1.1".to_string(),
                    vec![
                        "Host: 127.0.0.1".to_string(),
                        "Content-Length: 4".to_string(),
                        "Content-Type: application/json; charset=utf-8".to_string(),
                        "Accept: */*".to_string(),
                    ]
                ),
                (
                    "POST /get_o_indexes.bin HTTP/1.1".to_string(),
                    vec![
                        "Host: 127.0.0.1".to_string(),
                        "Content-Length: 3".to_string(),
                        "Accept: */*".to_string(),
                    ]
                ),
            ]
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn call_deadline_grows_with_the_reply_allowance() {
        let timeouts = HttpTimeouts::default();
        assert_eq!(timeouts.deadline(0), Duration::from_secs(210));
        assert_eq!(
            timeouts.deadline(MAX_RESPONSE_BYTES),
            Duration::from_secs(210 + 16 * 1024)
        );
    }

    #[test]
    fn getblocks_request_bytes_match_previous_backend() {
        // The epee request serialization must match what the
        // previous client sent.
        #[derive(Serialize, Debug)]
        struct Request {
            block_ids: Vec<u8>,
            start_height: u64,
            prune: bool,
            no_miner_tx: bool,
        }

        let hashes = [[7u8; 32], [9u8; 32]];
        let request = Request {
            block_ids: hashes.iter().flat_map(|h| h.iter().copied()).collect(),
            start_height: 12345,
            prune: false,
            no_miner_tx: false,
        };
        let bytes = monero_epee_bin_serde::to_bytes(&request).unwrap();
        // Round-trips through the epee parser with the blob intact
        #[derive(Deserialize, Debug)]
        struct Parsed {
            #[serde(with = "epee_byte_blob")]
            block_ids: Vec<u8>,
            start_height: u64,
            prune: bool,
            no_miner_tx: bool,
        }
        let parsed: Parsed = crate::safe_epee::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.block_ids.len(), 64);
        assert_eq!(parsed.start_height, 12345);
        assert!(!parsed.prune);
        assert!(!parsed.no_miner_tx);
    }

    #[test]
    fn parses_get_o_indexes_fixture() {
        // The captured binary fixture from a real daemon must parse with the
        // ported response structs.
        #[derive(serde::Deserialize)]
        struct RpcCall {
            route: String,
            response: String,
            is_binary: bool,
        }
        let vectors_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/vectors/honked_bagpipe_rpc.json");
        let calls: Vec<RpcCall> =
            serde_json::from_str(&std::fs::read_to_string(vectors_path).unwrap()).unwrap();
        let call = calls
            .iter()
            .find(|call| call.route == "get_o_indexes.bin")
            .unwrap();
        assert!(call.is_binary);

        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&call.response)
            .unwrap();

        #[allow(dead_code)]
        #[derive(Deserialize, Debug)]
        struct OIndexes {
            o_indexes: Vec<u64>,
        }
        let parsed: OIndexes = crate::safe_epee::from_bytes(&bytes).unwrap();
        assert!(!parsed.o_indexes.is_empty());
    }

    #[test]
    fn prefix_timelock_parses_variants() {
        // version 2, timelock 0
        assert_eq!(prefix_timelock(&[2, 0]).unwrap(), Timelock::None);
        // version 2, timelock 1000 (varint 0xe8 0x07)
        assert_eq!(
            prefix_timelock(&[2, 0xe8, 0x07]).unwrap(),
            Timelock::Block(1000)
        );
        // version 2, timelock at the block/time threshold
        let mut data = vec![2];
        monero_oxide::io::VarInt::write(&500_000_000u64, &mut data).unwrap();
        assert_eq!(prefix_timelock(&data).unwrap(), Timelock::Time(500_000_000));
    }

    #[tokio::test]
    async fn fee_priority_selects_the_daemons_wallet2_tier() {
        #[derive(Clone, Debug)]
        struct Connection;
        #[async_trait]
        impl RpcConnection for Connection {
            async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
                assert_eq!(route, "json_rpc");
                assert_eq!(
                    serde_json::from_slice::<Value>(&body).unwrap()["method"],
                    "get_fee_estimate"
                );
                Ok(serde_json::to_vec(&json!({
                    "result": {
                        "fee": 32_000,
                        "fees": [32_000, 130_000, 510_000, 6_400_000],
                        "quantization_mask": 10_000,
                        "status": "OK"
                    }
                }))
                .unwrap())
            }
        }

        let rpc = Rpc::new_with_connection(Connection);
        assert_eq!(
            rpc.get_fee_for_priority(FeePriority::Normal).await.unwrap(),
            Fee {
                per_weight: 130_000,
                mask: 10_000
            }
        );
    }

    fn monerod_backlog_reply(entries: &[(u64, u64)]) -> Vec<u8> {
        let mut reply = b"{\"id\": \"0\", \"jsonrpc\": \"2.0\", \"result\": {".to_vec();
        let pool: Vec<_> = entries.iter().filter(|(weight, _)| *weight != 0).collect();
        if !pool.is_empty() {
            reply.extend(b"\"backlog\": \"");
            for (weight, fee) in pool {
                for byte in [*weight, *fee, 0].into_iter().flat_map(u64::to_le_bytes) {
                    match byte {
                        0x08 => reply.extend(b"\\b"),
                        0x0c => reply.extend(b"\\f"),
                        b'\n' => reply.extend(b"\\n"),
                        b'\r' => reply.extend(b"\\r"),
                        b'\t' => reply.extend(b"\\t"),
                        0x0b => reply.extend(b"\\v"),
                        b'"' | b'\\' | b'/' => reply.extend([b'\\', byte]),
                        other => reply.push(other),
                    }
                }
            }
            reply.extend(b"\", ");
        }
        reply.extend(b"\"credits\": 0, \"status\": \"OK\", \"untrusted\": false}}");
        reply
    }

    #[test]
    fn txpool_backlog_is_read_from_monerods_blob() {
        let entry = |weight, fee| BacklogEntry { weight, fee };
        let entries = [
            (0x0c0b_0a09_0822_2f5c, 0xfffe_8000_0d00_5c5c),
            (150_000, 3_000_000_000),
        ];
        assert_eq!(
            parse_txpool_backlog(&monerod_backlog_reply(&entries)),
            Ok(vec![
                entry(entries[0].0, entries[0].1),
                entry(150_000, 3_000_000_000)
            ])
        );

        assert_eq!(
            parse_txpool_backlog(&monerod_backlog_reply(&[])),
            Ok(vec![])
        );
        assert_eq!(
            parse_txpool_backlog(br#"{"result":{"credits":0,"status":"OK"}}"#),
            Ok(vec![])
        );

        assert_eq!(
            parse_txpool_backlog(br#"{"result":{"backlog":"0123456789","status":"OK"}}"#),
            Ok(vec![])
        );
        assert_eq!(
            parse_txpool_backlog(
                br#"{"result":{"backlog":[{"weight":300000,"fee":1}],"status":"OK"}}"#
            ),
            Ok(vec![])
        );

        let mut escaped = br#"{"result":{"backlog":"\u00e9"#.to_vec();
        escaped.extend([b'x'; 22]);
        escaped.extend(br#"","status":"OK"}}"#);
        let parsed = parse_txpool_backlog(&escaped).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].weight.to_le_bytes()[..2], [0xc3, 0xa9]);

        assert!(parse_txpool_backlog(br#"{"result":{"backlog":"unterminated"#).is_err());
        assert!(parse_txpool_backlog(br#"{"error":{"code":-32601}}"#).is_err());
        assert!(parse_txpool_backlog(b"").is_err());
    }

    #[test]
    fn txpool_backlog_escapes_are_read_as_epee_reads_them() {
        let read = |escape: &str, decoded: usize| {
            let mut body = br#"{"result":{"backlog":""#.to_vec();
            body.extend(escape.as_bytes());
            body.extend(vec![b'x'; 24 - decoded]);
            body.extend(br#"","status":"OK"}}"#);
            parse_txpool_backlog(&body).map(|entries| {
                assert_eq!(entries.len(), 1, "{escape}");
                entries[0].weight.to_le_bytes()[..decoded].to_vec()
            })
        };
        for (escape, bytes) in [
            (r"\u0000", &[0x00][..]),
            (r"\u0041", &[0x41]),
            (r"\u00e9", &[0xc3, 0xa9]),
            (r"\u07FF", &[0xdf, 0xbf]),
            (r"\u0800", &[0xe0, 0xa0, 0x80]),
            (r"\ud800", &[0xed, 0xa0, 0x80]),
            (r"\uDFFF", &[0xed, 0xbf, 0xbf]),
            (r"\uffff", &[0xef, 0xbf, 0xbf]),
            (r"\ud83d\ude00", &[0xed, 0xa0, 0xbd, 0xed, 0xb8, 0x80]),
            (r"\v\'\/", &[0x0b, b'\'', b'/']),
        ] {
            assert_eq!(read(escape, bytes.len()), Ok(bytes.to_vec()), "{escape}");
        }

        let mut raw = br#"{"result":{"backlog":""#.to_vec();
        raw.extend([0xed, 0xa0, 0x80]);
        raw.extend([b'x'; 21]);
        raw.extend(br#"","status":"OK"}}"#);
        let mut escaped = br#"{"result":{"backlog":"\ud800"#.to_vec();
        escaped.extend([b'x'; 21]);
        escaped.extend(br#"","status":"OK"}}"#);
        assert_eq!(parse_txpool_backlog(&raw), parse_txpool_backlog(&escaped));
        assert_eq!(parse_txpool_backlog(&raw).unwrap().len(), 1);

        for escape in [r"\u+0e9", r"\u00g0", r"\u12", r"\x", r"\U0041"] {
            assert!(read(escape, 0).is_err(), "{escape}");
        }
        assert!(parse_txpool_backlog(br#"{"result":{"backlog":"\u00"#).is_err());

        let mut body = br#"{"result":{"back\u006cog":""#.to_vec();
        body.extend([b'x'; 24]);
        body.extend(br#"","st\u0061tus":"\u004fK","top_hash":"\ud800\v"#);
        body.extend([0x01, 0xff]);
        body.extend(br#"","untrusted":false}}"#);
        assert_eq!(parse_txpool_backlog(&body).unwrap().len(), 1);
        let mut body = br#"{"id":"backlog","result":{"backlog":""#.to_vec();
        body.extend([b'x'; 24]);
        body.extend(br#"","status":"OK"}}"#);
        assert_eq!(parse_txpool_backlog(&body).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn automatic_fee_matches_wallet2_backlog_and_block_fullness_rules() {
        #[derive(Clone, Debug)]
        struct Connection {
            backlog_weight: u64,
            block_weight: u64,
        }
        #[async_trait]
        impl RpcConnection for Connection {
            async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
                assert_eq!(route, "json_rpc");
                let request: Value = serde_json::from_slice(&body).unwrap();
                let result = match request["method"].as_str().unwrap() {
                    "get_fee_estimate" => json!({
                        "fee": 32_000,
                        "fees": [32_000, 130_000, 510_000, 6_400_000],
                        "quantization_mask": 10_000,
                        "status": "OK"
                    }),
                    "get_txpool_backlog" => {
                        return Ok(monerod_backlog_reply(&[(
                            self.backlog_weight,
                            self.backlog_weight * 32_000,
                        )]))
                    }
                    "get_info" => json!({
                        "height": 100,
                        "block_weight_limit": 600_000,
                        "status": "OK"
                    }),
                    "getblockheadersrange" => json!({
                        "headers": (0..10)
                            .map(|_| json!({ "block_weight": self.block_weight }))
                            .collect::<Vec<_>>(),
                        "status": "OK"
                    }),
                    method => panic!("unexpected method {method}"),
                };
                Ok(serde_json::to_vec(&json!({ "result": result })).unwrap())
            }
        }

        let fee = |backlog_weight, block_weight| async move {
            Rpc::new_with_connection(Connection {
                backlog_weight,
                block_weight,
            })
            .get_fee()
            .await
            .unwrap()
            .per_weight
        };

        assert_eq!(fee(0, 240_000).await, 32_000);
        assert_eq!(fee(0, 243_000).await, 130_000);
        assert_eq!(fee(300_000, 0).await, 130_000);
    }

    #[test]
    fn node_answers_are_kept_as_long_as_wallet2_keeps_them() {
        let estimate = FeeEstimate {
            mask: 10_000,
            tiers: vec![32_000, 130_000, 510_000, 6_400_000],
        };
        let mut state = NodeCacheState::default();
        assert_eq!(state.fee_estimate(Some(500), Some(1_000)), None);

        state.fee_estimate = Some((Some(500), Some(1_000), estimate.clone()));
        assert_eq!(
            state.fee_estimate(Some(500), Some(1_000)),
            Some(estimate.clone())
        );
        assert_eq!(
            state.fee_estimate(Some(500), Some(9_999)),
            Some(estimate.clone())
        );
        assert_eq!(state.fee_estimate(Some(500), None), Some(estimate.clone()));
        assert_eq!(state.fee_estimate(Some(501), Some(1_000)), None);
        assert_eq!(state.fee_estimate(None, Some(1_000)), None);

        state.fee_estimate = Some((None, Some(1_000), estimate.clone()));
        assert_eq!(
            state.fee_estimate(None, Some(1_029)),
            Some(estimate.clone())
        );
        assert_eq!(state.fee_estimate(None, Some(1_030)), None);
        assert_eq!(
            state.fee_estimate(None, Some(999)),
            None,
            "a clock that went back"
        );
        assert_eq!(state.fee_estimate(None, None), None, "no clock");
        assert_eq!(state.fee_estimate(Some(500), Some(1_000)), None);
        state.fee_estimate = Some((None, None, estimate));
        assert_eq!(state.fee_estimate(None, Some(1_000)), None);

        let info = NodeInfo {
            height: 500,
            block_weight_limit: 600_000,
        };
        state.info = Some((1_000, info));
        assert_eq!(state.info(Some(1_029)), Some(info));
        assert_eq!(state.info(Some(1_030)), None);
        assert_eq!(state.info(None), None);

        assert!(state.wallet_height_is_current(500, Some(1_000)));
        assert!(state.wallet_height_is_current(500, Some(1_029)));
        assert!(!state.wallet_height_is_current(500, Some(1_030)));
        assert!(!state.wallet_height_is_current(500, Some(5_000)));
        assert!(state.wallet_height_is_current(501, Some(5_000)));
        assert!(!state.wallet_height_is_current(501, None), "no clock");
        assert!(!state.wallet_height_is_current(502, None), "no clock");
    }

    #[tokio::test]
    async fn a_fee_estimate_for_a_stale_wallet_height_follows_the_node() {
        use std::sync::atomic::{AtomicU64, Ordering};

        #[derive(Clone, Debug, Default)]
        struct Connection {
            height: Arc<AtomicU64>,
            calls: Arc<Mutex<Vec<String>>>,
        }
        #[async_trait]
        impl RpcConnection for Connection {
            async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
                assert_eq!(route, "json_rpc");
                let request: Value = serde_json::from_slice(&body).unwrap();
                let method = request["method"].as_str().unwrap().to_string();
                let height = self.height.load(Ordering::SeqCst);
                let result = match method.as_str() {
                    "get_fee_estimate" => json!({
                        "fee": 20_000 + height,
                        "fees": [20_000 + height, 80_000, 320_000, 4_000_000],
                        "quantization_mask": 10_000,
                        "status": "OK"
                    }),
                    "get_info" => json!({
                        "height": height,
                        "block_weight_limit": 600_000,
                        "status": "OK"
                    }),
                    method => panic!("unexpected method {method}"),
                };
                self.calls.lock().unwrap().push(method);
                Ok(serde_json::to_vec(&json!({ "result": result })).unwrap())
            }
        }

        let connection = Connection::default();
        connection.height.store(100, Ordering::SeqCst);
        let rpc = Rpc::new_with_connection(connection.clone());
        let fee = |wallet_height| {
            let rpc = rpc.clone();
            async move {
                rpc.get_fee_for_priority_at_height(FeePriority::Low, Some(wallet_height))
                    .await
                    .unwrap()
                    .per_weight
            }
        };
        let calls = || std::mem::take(&mut *connection.calls.lock().unwrap());
        let age = || {
            let mut state = rpc.1.state();
            if let Some((_, since)) = &mut state.wallet_height {
                *since -= 31;
            }
            if let Some((at, _)) = &mut state.info {
                *at -= 31;
            }
        };

        assert_eq!(fee(100).await, 20_100);
        assert_eq!(fee(100).await, 20_100);
        assert_eq!(calls(), ["get_fee_estimate"]);

        age();
        assert_eq!(fee(100).await, 20_100);
        assert_eq!(calls(), ["get_info"]);

        connection.height.store(101, Ordering::SeqCst);
        age();
        assert_eq!(fee(100).await, 20_101);
        assert_eq!(calls(), ["get_info", "get_fee_estimate"]);
        assert_eq!(fee(100).await, 20_101);
        assert_eq!(calls(), Vec::<String>::new());

        assert_eq!(fee(101).await, 20_101);
        assert_eq!(calls(), Vec::<String>::new());
        connection.height.store(102, Ordering::SeqCst);
        assert_eq!(fee(102).await, 20_102);
        assert_eq!(calls(), ["get_fee_estimate"]);
    }

    #[test]
    fn tier_bounds_are_the_daemons_formula_at_its_maximum() {
        fn round_up(amount: u128) -> u64 {
            let mut unit = 1;
            while amount / unit >= 100 {
                unit *= 10;
            }
            u64::try_from(amount.div_ceil(unit) * unit).unwrap()
        }
        let reward = u128::from(u64::MAX >> 19);
        let zone = 300_000u128;
        let reference = 3_000u128;
        let low = reward * reference / (zone * zone);
        let normal = 4 * reward * reference / (zone * zone);
        let elevated = 16 * reward * reference / (zone * zone);
        let priority = (4 * elevated).max(4 * elevated * zone / (32 * reference * zone / zone));
        assert_eq!(
            [low, normal, elevated, priority].map(round_up),
            MAX_DAEMON_FEE_PER_BYTE
        );
        assert_eq!(DEFAULT_MAX_FEE_PER_BYTE, 240_000_000);
    }

    #[tokio::test]
    async fn checked_fee_bounds_the_quantization_mask() {
        #[derive(Clone, Debug)]
        struct Connection(u64);
        #[async_trait]
        impl RpcConnection for Connection {
            async fn post(&self, _: &str, _: Vec<u8>) -> Result<Vec<u8>, RpcError> {
                Ok(serde_json::to_vec(&json!({ "result": {
                    "fee": 32_000,
                    "fees": [32_000, 130_000, 510_000, 6_400_000],
                    "quantization_mask": self.0,
                    "status": "OK"
                } }))
                .unwrap())
            }
        }
        let checked = |mask, limit| async move {
            Rpc::new_with_connection(Connection(mask))
                .get_fee_checked_for_priority(FeePriority::Low, limit)
                .await
        };

        let (display_decimal_point, quantization_decimals) = (12, 8);
        assert_eq!(
            MAX_DAEMON_FEE_QUANTIZATION_MASK,
            10u64.pow(display_decimal_point - quantization_decimals)
        );

        let fee = |mask| Fee {
            per_weight: 32_000,
            mask,
        };
        let refused = |mask| {
            Err(RpcError::MaskExceedsLimit {
                mask,
                limit: MAX_DAEMON_FEE_QUANTIZATION_MASK,
            })
        };
        assert_eq!(checked(10_000, 32_000).await, Ok(fee(10_000)));
        assert_eq!(checked(1, 32_000).await, Ok(fee(1)));
        assert_eq!(checked(10_001, 32_000).await, refused(10_001));
        for limit in [DEFAULT_MAX_FEE_PER_BYTE, u64::MAX] {
            assert_eq!(checked(10_001, limit).await, refused(10_001));
            assert_eq!(checked(32_000, limit).await, refused(32_000));
            assert_eq!(checked(240_000_000, limit).await, refused(240_000_000));
            assert_eq!(checked(u64::MAX, limit).await, refused(u64::MAX));
        }
    }

    #[tokio::test]
    async fn automatic_fee_checks_blocks_at_the_wallet_height() {
        #[derive(Clone, Debug)]
        struct Connection;
        #[async_trait]
        impl RpcConnection for Connection {
            async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
                assert_eq!(route, "json_rpc");
                let request: Value = serde_json::from_slice(&body).unwrap();
                let result = match request["method"].as_str().unwrap() {
                    "get_fee_estimate" => json!({
                        "fee": 32_000,
                        "fees": [32_000, 130_000, 510_000, 6_400_000],
                        "quantization_mask": 10_000,
                        "status": "OK"
                    }),
                    "get_txpool_backlog" => return Ok(monerod_backlog_reply(&[])),
                    "get_info" => json!({
                        "height": 1_000,
                        "block_weight_limit": 600_000,
                        "status": "OK"
                    }),
                    "getblockheadersrange" => {
                        assert_eq!(request["params"]["start_height"], 490);
                        assert_eq!(request["params"]["end_height"], 499);
                        json!({
                            "headers": (0..10)
                                .map(|_| json!({ "block_weight": 0 }))
                                .collect::<Vec<_>>(),
                            "status": "OK"
                        })
                    }
                    method => panic!("unexpected method {method}"),
                };
                Ok(serde_json::to_vec(&json!({ "result": result })).unwrap())
            }
        }

        assert_eq!(
            Rpc::new_with_connection(Connection)
                .get_fee_for_priority_at_height(FeePriority::Automatic, Some(500))
                .await
                .unwrap()
                .per_weight,
            32_000
        );
    }
}
