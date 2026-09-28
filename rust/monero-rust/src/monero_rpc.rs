//! Monero daemon RPC client, ported from the vendored serai mirror onto the
//! monero-oxide types.
//!
//! monero-oxide's `interface` crate is trait-only: it ships no HTTP client,
//! so this module supplies a generic connection interface and native HTTP transport.
//! It supports JSON-RPC, binary Epee endpoints, `/getblocks.bin` output indices,
//! pruned transactions, and digest authentication.

#![allow(dead_code)]

use std::fmt::Debug;

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
}
#[derive(Deserialize, Debug)]
struct TransactionsResponse {
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
    outputs: Vec<GetOutputsOut>,
    get_txid: bool,
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

#[derive(Clone, PartialEq, Eq, Debug, Error)]
pub enum RpcError {
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
    #[error("pruned transaction")]
    PrunedTransaction,
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
    #[error("node answered with status {0:?}")]
    NodeStatus(String),
    #[error("invalid fee estimate ({0})")]
    InvalidFeeEstimate(&'static str),
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
        Err(RpcError::NodeStatus(status.to_string()))
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
    let reply: JsonRpcResponse<Status> = serde_json::from_slice(&json)
        .map_err(|_| RpcError::InternalError("Failed to parse JSON response"))?;
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
}

#[cfg(target_arch = "wasm32")]
#[async_trait(?Send)]
pub trait RpcConnection: Clone + Debug {
    /// Perform a POST request to the specified route with the specified body.
    ///
    /// The implementor is left to handle anything such as authentication.
    async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError>;
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug)]
pub struct HttpRpc {
    client: Client,
    userpass: Option<(String, String)>,
    url: String,
}

#[cfg(not(target_arch = "wasm32"))]
impl HttpRpc {
    /// Create a new HTTP(S) RPC connection.
    ///
    /// A daemon requiring authentication can be used via including the
    /// username and password in the URL.
    pub fn new(mut url: String) -> Result<Rpc<HttpRpc>, RpcError> {
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

        Ok(Rpc(HttpRpc {
            client: Client::new(),
            userpass,
            url,
        }))
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[async_trait]
impl RpcConnection for HttpRpc {
    async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
        let mut builder = self.client.post(self.url.clone() + "/" + route).body(body);

        if let Some((user, pass)) = &self.userpass {
            let req = self
                .client
                .post(&self.url)
                .send()
                .await
                .map_err(|_| RpcError::InvalidNode)?;
            // Only provide authentication if this daemon actually expects it
            if let Some(header) = req.headers().get("www-authenticate") {
                builder = builder.header(
                    "Authorization",
                    digest_auth::parse(header.to_str().map_err(|_| RpcError::InvalidNode)?)
                        .map_err(|_| RpcError::InvalidNode)?
                        .respond(&AuthContext::new_post::<_, _, _, &[u8]>(
                            user,
                            pass,
                            "/".to_string() + route,
                            None,
                        ))
                        .map_err(|_| RpcError::InvalidNode)?
                        .to_header_string(),
                );
            }
        }

        Ok(builder
            .send()
            .await
            .map_err(|_| RpcError::ConnectionError)?
            .bytes()
            .await
            .map_err(|_| RpcError::ConnectionError)?
            .slice(..)
            .to_vec())
    }
}

#[derive(Clone, Debug)]
pub struct Rpc<R: RpcConnection>(R);
impl<R: RpcConnection> Rpc<R> {
    /// Create a new Rpc instance from any RpcConnection implementation.
    pub fn new_with_connection(connection: R) -> Self {
        Rpc(connection)
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
        self.call_tail(
            route,
            self.0
                .post(
                    route,
                    if let Some(params) = params {
                        serde_json::to_vec(&params).map_err(|_| {
                            RpcError::InternalError("Failed to serialize JSON request")
                        })?
                    } else {
                        vec![]
                    },
                )
                .await?,
        )
        .await
    }

    /// Perform a JSON-RPC call with the specified method with the provided parameters.
    pub async fn json_rpc_call<Response: DeserializeOwned + Debug>(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Response, RpcError> {
        let mut req = json!({ "method": method });
        if let Some(params) = params {
            req.as_object_mut()
                .ok_or(RpcError::InternalError("Failed to build JSON-RPC request"))?
                .insert("params".into(), params);
        }
        Ok(self
            .rpc_call::<_, JsonRpcResponse<Response>>("json_rpc", Some(req))
            .await?
            .result)
    }

    /// Perform a binary call to the specified route with the provided parameters.
    pub async fn bin_call<Response: DeserializeOwned + Debug>(
        &self,
        route: &str,
        params: Vec<u8>,
    ) -> Result<Response, RpcError> {
        self.call_tail(route, self.0.post(route, params).await?)
            .await
    }

    async fn call_tail<Response: DeserializeOwned + Debug>(
        &self,
        route: &str,
        res: Vec<u8>,
    ) -> Result<Response, RpcError> {
        Ok(if !route.ends_with(".bin") {
            serde_json::from_str(std::str::from_utf8(&res).map_err(|_| RpcError::InvalidNode)?)
                .map_err(|_| RpcError::InternalError("Failed to parse JSON response"))?
        } else {
            crate::safe_epee::from_bytes(&res)
                .map_err(|_| RpcError::InternalError("Failed to parse binary response"))?
        })
    }

    /// The RingCT type of a transaction built under the node's current hard fork.
    pub async fn get_rct_type(&self) -> Result<RctType, RpcError> {
        #[derive(Deserialize, Debug)]
        struct ProtocolResponse {
            major_version: usize,
        }

        #[derive(Deserialize, Debug)]
        struct LastHeaderResponse {
            block_header: ProtocolResponse,
        }

        Ok(
            match self
                .json_rpc_call::<LastHeaderResponse>("get_last_block_header", None)
                .await?
                .block_header
                .major_version
            {
                13 | 14 => RctType::ClsagBulletproof,
                15 | 16 => RctType::ClsagBulletproofPlus,
                protocol => Err(RpcError::UnsupportedProtocol(protocol))?,
            },
        )
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

    pub async fn get_transactions(
        &self,
        hashes: &[[u8; 32]],
    ) -> Result<Vec<Transaction>, RpcError> {
        if hashes.is_empty() {
            return Ok(vec![]);
        }

        let txs: TransactionsResponse = self
            .rpc_call(
                "get_transactions",
                Some(json!({
                  "txs_hashes": hashes.iter().map(hex::encode).collect::<Vec<_>>()
                })),
            )
            .await?;

        if !txs.missed_tx.is_empty() {
            Err(RpcError::TransactionsNotFound(
                txs.missed_tx
                    .iter()
                    .map(|hash| hash_hex(hash))
                    .collect::<Result<_, _>>()?,
            ))?;
        }

        // Require one response per requested hash. Per-entry hash checks alone would
        // accept a truncated reply and let the scanner miss received outputs or spent
        // key images. Extra entries would index past `hashes`.
        if txs.txs.len() != hashes.len() {
            return Err(RpcError::InvalidNode);
        }

        txs.txs
            .iter()
            .enumerate()
            .map(|(i, res)| {
                let tx = Transaction::<NotPruned>::read::<&[u8]>(
                    &mut rpc_hex(if !res.as_hex.is_empty() {
                        &res.as_hex
                    } else {
                        &res.pruned_as_hex
                    })?
                    .as_ref(),
                )
                .map_err(|_| match hash_hex(&res.tx_hash) {
                    Ok(hash) => RpcError::InvalidTransaction(hash),
                    Err(err) => err,
                })?;

                // https://github.com/monero-project/monero/issues/8311
                if res.as_hex.is_empty() {
                    match tx.prefix().inputs.first() {
                        Some(Input::Gen { .. }) => (),
                        _ => Err(RpcError::PrunedTransaction)?,
                    }
                }

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

        let txs: TransactionsResponse = self
            .rpc_call(
                "get_transactions",
                Some(json!({
                  "txs_hashes": hashes.iter().map(hex::encode).collect::<Vec<_>>()
                })),
            )
            .await?;

        if !txs.missed_tx.is_empty() {
            Err(RpcError::TransactionsNotFound(
                txs.missed_tx
                    .iter()
                    .map(|hash| hash_hex(hash))
                    .collect::<Result<_, _>>()?,
            ))?;
        }

        // Callers match timelocks to transactions by position, so the counts must agree.
        if txs.txs.len() != hashes.len() {
            return Err(RpcError::InvalidNode);
        }

        txs.txs
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
                monero_epee_bin_serde::to_bytes(&Request { txid: hash }).map_err(|_| {
                    RpcError::InternalError("epee serialization failed for get_o_indexes request")
                })?,
            )
            .await?;

        Ok(indexes.o_indexes)
    }

    /// Get the output distribution, from the specified height to the
    /// specified height (both inclusive).
    pub async fn get_output_distribution(
        &self,
        _from: usize,
        _to: usize,
    ) -> Result<Vec<u64>, RpcError> {
        let response: OutputDistributionResponse = self
            .bin_call(
                "get_output_distribution.bin",
                monero_epee_bin_serde::to_bytes(&OutputDistributionRequest {
                    amounts: vec![0],
                    compress: true,
                })
                .map_err(|_| {
                    RpcError::InternalError(
                        "epee serialization failed for output distribution request",
                    )
                })?,
            )
            .await?;
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
        Ok(result)
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
                .bin_call(
                    "get_outs.bin",
                    monero_epee_bin_serde::to_bytes(&request).map_err(|_| {
                        RpcError::InternalError("epee serialization failed for get_outs request")
                    })?,
                )
                .await?;

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
        struct InfoResponse {
            height: u64,
            #[serde(default)]
            block_weight_limit: u64,
            #[serde(default)]
            block_size_limit: u64,
            #[serde(default)]
            status: String,
        }
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

        let request = serde_json::to_vec(&json!({ "method": "get_txpool_backlog", "params": {} }))
            .map_err(|_| RpcError::InternalError("Failed to serialize JSON request"))?;
        let backlog = parse_txpool_backlog(&self.0.post("json_rpc", request).await?)?;
        let info: InfoResponse = self.json_rpc_call("get_info", Some(json!({}))).await?;
        require_ok_status(&info.status)?;
        let block_weight_limit = if info.block_weight_limit == 0 {
            info.block_size_limit
        } else {
            info.block_weight_limit
        };
        let full_reward_zone = block_weight_limit / 2;
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

    async fn fee_tier_for_priority_at_height(
        &self,
        priority: FeePriority,
        wallet_height: Option<u64>,
    ) -> Result<(usize, Fee), RpcError> {
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

        let res: SendRawResponse = self
            .rpc_call(
                "send_raw_transaction",
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

    pub async fn get_transaction_pool(
        &self,
    ) -> Result<(Vec<Transaction>, Vec<[u8; 32]>), RpcError> {
        #[derive(Deserialize, Debug)]
        struct PoolTransaction {
            id_hash: String,
            tx_blob: String,
        }

        #[derive(Deserialize, Debug)]
        struct SpentKeyImage {
            id_hash: String,
        }

        #[allow(dead_code)]
        #[derive(Deserialize, Debug)]
        struct TransactionPoolResponse {
            #[serde(default)]
            transactions: Vec<PoolTransaction>,
            #[serde(default)]
            spent_key_images: Vec<SpentKeyImage>,
            status: String,
        }

        let res: TransactionPoolResponse = self
            .rpc_call::<Option<()>, _>("get_transaction_pool", None)
            .await?;

        if res.status != "OK" {
            return Err(RpcError::InvalidNode);
        }

        let transactions = res
            .transactions
            .iter()
            .map(|pool_tx| {
                let tx_bytes = rpc_hex(&pool_tx.tx_blob)?;
                let tx = Transaction::<NotPruned>::read::<&[u8]>(&mut tx_bytes.as_ref())
                    .map_err(|_| RpcError::InvalidNode)?;

                let expected_hash = hash_hex(&pool_tx.id_hash)?;
                if tx.hash() != expected_hash {
                    return Err(RpcError::InvalidNode);
                }

                Ok(tx)
            })
            .collect::<Result<Vec<_>, RpcError>>()?;

        let spent_key_images = res
            .spent_key_images
            .iter()
            .map(|ki| hash_hex(&ki.id_hash))
            .collect::<Result<Vec<_>, RpcError>>()?;

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
        // block_ids must be serialized as a contiguous blob
        // (KV_SERIALIZE_CONTAINER_POD_AS_BLOB)
        let block_ids_blob: Vec<u8> = block_ids.iter().flat_map(|h| h.iter().copied()).collect();

        #[derive(Serialize, Debug)]
        struct Request {
            block_ids: Vec<u8>,
            start_height: u64,
            prune: bool,
            no_miner_tx: bool,
        }

        let req = Request {
            block_ids: block_ids_blob,
            start_height,
            prune,
            no_miner_tx: false,
        };

        let res: GetBlocksFastResponse = self
            .bin_call(
                "getblocks.bin",
                monero_epee_bin_serde::to_bytes(&req).map_err(|_| {
                    RpcError::InternalError("epee serialization failed for getblocks.bin request")
                })?,
            )
            .await?;

        if res.status != "OK" {
            return Err(RpcError::InvalidNode);
        }

        Ok(res)
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
    /// Raw transaction blobs (full transactions when prune=false).
    #[serde(default, with = "vec_of_bytes")]
    pub txs: Vec<Vec<u8>>,
    /// Whether the transaction data is pruned.
    #[serde(default)]
    pub pruned: bool,
    /// Block weight.
    #[serde(default)]
    pub block_weight: u64,
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
