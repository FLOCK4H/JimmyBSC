use crate::libs::lookup::save_log_to_file;
use alloy::rpc::types::eth::{Filter, Log as RpcLog};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;

///! The [`BscClient`] type encapsulates a connection to a BNB Smart
///! Chain node and exposes a small set of async helpers implemented
///! over plain JSON‑RPC using `reqwest`. We only depend on Alloy for
///! types and key handling, ensuring a consistent 0.7 series across the
///! workspace to avoid native `ckzg` link conflicts.
///! GPT-5 AGENT GENERATED FILE
use alloy::primitives::{Address, U256};
use alloy::signers::local::PrivateKeySigner;
use anyhow::{Context, Result};
use reqwest::Client as HttpClient;

/// A high level client for interacting with the BNB Smart Chain via
/// JSON‑RPC.  The client wraps an underlying [`ReqwestClient`]
/// transport provided by the Alloy project and exposes idiomatic
/// methods for all BSC specific RPC calls.  It also retains the
/// address derived from the private key so that helper functions
/// living in [`crate::libs::bsc::spells`] can use it directly.
#[derive(Clone, Debug)]
pub struct BscClient {
    rpc_url: Url,
    http: HttpClient,
    /// The Ethereum style address derived from the configured
    /// private key.  Exposed as a public field so callers may
    /// reference it directly when constructing parameters.
    pub address: Address,
}

#[derive(Clone, Debug)]
pub struct BscWsClient {
    ws_url: Url,
    pub address: alloy::primitives::Address,
}

impl BscClient {
    /// * `PRIVATE_KEY` – the hex encoded 32‑byte private key used to
    ///   derive the default account address.  This value is
    ///   required; if absent an error is returned.
    ///
    /// When compiled with the `dotenv` feature this method will
    /// automatically load a `.env` file from the current working
    /// directory before reading variables.  This mirrors the
    /// behaviour of popular `.env` loaders and simplifies local
    /// development.
    pub async fn new(rpc_url: String, private_key: String) -> Result<Self> {
        // Read the RPC endpoint URL from the environment or fall
        // back to the public BSC node.  The provided URL must be
        // parseable by the `url` crate; otherwise an error will
        // propagate to the caller.
        let url = Url::parse(&rpc_url)
            .with_context(|| format!("Failed to parse BSC_RPC_URL: {rpc_url}"))?;

        // A private key is mandatory.  It is used only to derive
        // the associated address; no signing is performed by the
        // client itself.  Using a local signer from alloy-signer-local
        // allows us to easily derive the address.  If the key is
        // malformed (e.g. wrong length or non‑hex characters) an
        // informative error is returned.
        let signer: PrivateKeySigner = private_key
            .parse()
            .with_context(|| "PRIVATE_KEY did not contain a valid hex encoded secret")?;
        let address = signer.address();

        let http = HttpClient::new();

        Ok(Self {
            rpc_url: url,
            http,
            address,
        })
    }

    /// Return the numeric chain identifier for the remote node.  On
    /// BSC this value is expected to be `56`.  The method
    /// internally performs an `eth_chainId` call and converts the
    /// returned 64‑bit value into a native `u64`.  Any transport
    /// errors or JSON decoding failures will bubble up as an
    /// [`anyhow::Error`].
    pub async fn chain_id(&self) -> Result<u64> {
        let res = self.rpc("eth_chainId", serde_json::json!([])).await?;
        let hex = res.as_str().context("chainId not string")?;
        let val =
            u64::from_str_radix(hex.trim_start_matches("0x"), 16).context("bad chainId hex")?;
        Ok(val)
    }

    /// Issue an arbitrary JSON‑RPC call and receive a `serde_json::Value`
    /// as the response.  This helper can be used to access
    /// experimental or otherwise unsupported methods without having to
    /// define a new strongly typed wrapper.  Parameters must be
    /// provided as a `Vec<Value>` and correspond exactly to the
    /// expected RPC parameters.  If the call returns an error the
    /// [`anyhow::Error`] will contain the underlying RPC failure.
    pub async fn raw_call(&self, method: &str, params: Vec<Value>) -> Result<Value> {
        self.rpc(method, Value::Array(params)).await
    }

    /// Get a block header by number or tag.  The `block` argument may
    /// be a hex encoded quantity (e.g. `"0x10"`), a block hash, or
    /// one of the special tags: `"earliest"`, `"latest"`,
    /// `"safe"` or `"finalized"`.  The returned value is the
    /// decoded JSON representation of the block header.
    pub async fn get_header_by_number(&self, block: impl Into<String>) -> Result<Value> {
        self.rpc("eth_getHeaderByNumber", serde_json::json!([block.into()]))
            .await
    }

    /// Get a block by number or tag.  If `full_transactions` is
    /// `true` the returned block will include full transaction
    /// objects; otherwise only transaction hashes are returned.  The
    /// `block` argument accepts the same values as
    /// [`get_header_by_number`](Self::get_header_by_number).
    pub async fn get_block_by_number(
        &self,
        block: impl Into<String>,
        full_transactions: bool,
    ) -> Result<Value> {
        self.rpc(
            "eth_getBlockByNumber",
            serde_json::json!([block.into(), full_transactions]),
        )
        .await
    }

    /// Create a new finalized header filter.  The BSC node will
    /// return a filter identifier that can subsequently be passed to
    /// [`get_filter_changes`](Self::get_filter_changes) to retrieve
    /// hashes of newly finalized blocks.  The filter expires after a
    /// short period on the server side.
    pub async fn new_finalized_header_filter(&self) -> Result<Value> {
        self.rpc("eth_newFinalizedHeaderFilter", serde_json::json!([]))
            .await
    }

    /// Poll for changes on a previously created filter.  The
    /// `filter_id` must have been obtained from
    /// [`new_finalized_header_filter`](Self::new_finalized_header_filter).
    /// The returned vector contains raw block hashes as JSON values.
    pub async fn get_filter_changes(&self, filter_id: &str) -> Result<Vec<Value>> {
        let res = self
            .rpc("eth_getFilterChanges", serde_json::json!([filter_id]))
            .await?;
        let arr = res.as_array().cloned().unwrap_or_default();
        Ok(arr)
    }

    /// Retrieve the highest finalized header given a security level.
    /// The `verified_validator_num` parameter selects how many
    /// validators must have attested to the block: use a positive
    /// integer to require that many signatures or one of the
    /// convenience values `-1` (≥½ of validators), `-2` (≥⅔) or
    /// `-3` (all).  The returned header is guaranteed to advance
    /// monotonically with subsequent calls.
    pub async fn get_finalized_header(&self, verified_validator_num: i64) -> Result<Value> {
        self.rpc(
            "eth_getFinalizedHeader",
            serde_json::json!([verified_validator_num]),
        )
        .await
    }

    /// Retrieve the highest finalized block given a security level.
    /// When `full_transactions` is true the complete list of
    /// transactions is included in the response; when false only
    /// hashes are returned.  See
    /// [`get_finalized_header`](Self::get_finalized_header) for a
    /// description of `verified_validator_num`.
    pub async fn get_finalized_block(
        &self,
        verified_validator_num: i64,
        full_transactions: bool,
    ) -> Result<Value> {
        self.rpc(
            "eth_getFinalizedBlock",
            serde_json::json!([verified_validator_num, full_transactions]),
        )
        .await
    }

    /// Fetch the blob sidecar associated with a transaction hash.  If
    /// `full_blob` is provided and set to `false` only the first 32
    /// bytes of each blob are returned.  Passing `None` leaves the
    /// default server behaviour unchanged.
    pub async fn get_blob_sidecar_by_tx_hash(
        &self,
        tx_hash: &str,
        full_blob: Option<bool>,
    ) -> Result<Value> {
        let params = match full_blob {
            Some(b) => serde_json::json!([tx_hash, b]),
            None => serde_json::json!([tx_hash]),
        };
        self.rpc("eth_getBlobSidecarByTxHash", params).await
    }

    /// Fetch blob sidecars for a block specified by number, hash or tag.
    /// When `full_blob` is provided and set to `false` only the first
    /// 32 bytes of each blob are returned.  Passing `None` leaves
    /// the default server behaviour unchanged.
    pub async fn get_blob_sidecars(
        &self,
        block: impl Into<String>,
        full_blob: Option<bool>,
    ) -> Result<Value> {
        let params = match full_blob {
            Some(b) => serde_json::json!([block.into(), b]),
            None => serde_json::json!([block.into()]),
        };
        self.rpc("eth_getBlobSidecars", params).await
    }

    /// Perform a simple health check of the remote node.  Returns
    /// `true` if the node is healthy and serving requests, `false`
    /// otherwise.  This method maps directly onto the `eth_health`
    /// RPC method provided by BSC.
    pub async fn health(&self) -> Result<bool> {
        let res = self.rpc("eth_health", serde_json::json!([])).await?;
        Ok(res.as_bool().unwrap_or(true))
    }

    /// Fetch all transactions contained in the specified block.  The
    /// `block` parameter accepts a hex encoded quantity, a block
    /// hash, or one of the standard tags (e.g. `"latest"`).  The
    /// return value is a vector of raw JSON transaction objects.
    pub async fn get_transactions_by_block_number(
        &self,
        block: impl Into<String>,
    ) -> Result<Vec<Value>> {
        let res = self
            .rpc(
                "eth_getTransactionsByBlockNumber",
                serde_json::json!([block.into()]),
            )
            .await?;
        Ok(res.as_array().cloned().unwrap_or_default())
    }

    /// Retrieve both the original transaction data and the receipt for
    /// a given transaction hash.  The return value is a JSON object
    /// containing `transaction` and `receipt` fields.  The RPC method
    /// name is `eth_getTransactionDataAndReceipt`.
    pub async fn get_transaction_data_and_receipt(&self, tx_hash: &str) -> Result<Value> {
        self.rpc(
            "eth_getTransactionDataAndReceipt",
            serde_json::json!([tx_hash]),
        )
        .await
    }

    /// Convenience wrapper around `eth_getBalance`.  Returns the
    /// balance of the given `address` at the specified block tag.
    /// The default block tag is `"latest"`.  This method yields a
    /// [`U256`] which represents the balance in wei.  See
    /// [`crate::libs::bsc::spells::format_bnb`] for formatting the
    /// result into human readable BNB.
    pub async fn get_balance_at(&self, address: Address, block: &str) -> Result<U256> {
        let res = self
            .rpc("eth_getBalance", serde_json::json!([address, block]))
            .await?;
        let hex = res.as_str().context("balance not string")?;
        let mut stripped = hex.trim_start_matches("0x");

        // Pad to even length if needed (RPC can return "0x0", "0x1", etc.)
        let padded;
        if stripped.len() % 2 == 1 {
            padded = format!("0{}", stripped);
            stripped = &padded;
        }

        let bytes = hex::decode(stripped).context("bad balance hex")?;
        Ok(U256::from_be_slice(&bytes))
    }

    async fn rpc(&self, method: &str, params: Value) -> Result<Value> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });
        let res = self
            .http
            .post(self.rpc_url.as_str())
            .json(&body)
            .send()
            .await?;
        let status = res.status();
        let bytes = res.bytes().await?;
        // Try to decode JSON; if it fails, surface useful diagnostics
        let v: Value = match serde_json::from_slice(&bytes) {
            Ok(json) => json,
            Err(e) => {
                let mut sample = String::from_utf8_lossy(&bytes).to_string();
                if sample.len() > 512 {
                    sample.truncate(512);
                }
                // common provider misconfig: HTML/empty response or wrong endpoint (e.g. WSS passed as HTTP)
                let hint = if sample.trim_start().starts_with('<') {
                    "Response looks like HTML; your BSC_RPC may be an HTTP gateway page or blocked. Ensure it is a valid HTTPS JSON-RPC endpoint."
                } else if sample.trim().is_empty() {
                    "Empty body from RPC. Endpoint may be down or require authentication."
                } else {
                    "Non‑JSON response from RPC."
                };
                return Err(anyhow::anyhow!(
                    "RPC decode error ({status}): {e}. {hint}\nEndpoint: {}\nSample: {}",
                    self.rpc_url,
                    sample
                ));
            }
        };
        if let Some(err) = v.get("error") {
            anyhow::bail!("rpc error: {}", err);
        }
        Ok(v.get("result").cloned().unwrap_or(Value::Null))
    }
}

impl BscWsClient {
    pub async fn new(ws_url: String, private_key: String) -> anyhow::Result<Self> {
        use alloy::signers::local::PrivateKeySigner;
        let url = Url::parse(&ws_url)
            .map_err(|e| anyhow::anyhow!("Failed to parse BSC_WSS `{ws_url}`: {e}"))?;
        let signer: PrivateKeySigner = private_key.parse().map_err(|_| {
            anyhow::anyhow!("PRIVATE_KEY did not contain a valid hex encoded secret")
        })?;
        Ok(Self {
            ws_url: url,
            address: signer.address(),
        })
    }

    /// expose url so we can branch in ws/pairs.rs if needed
    pub fn url(&self) -> &Url {
        &self.ws_url
    }

    /// Old API – single subscription (kept for backward compatibility)
    pub async fn subscribe_logs(
        &self,
        filter: Filter,
    ) -> anyhow::Result<(mpsc::Receiver<RpcLog>, JoinHandle<()>)> {
        let (tx, rx) = mpsc::channel::<RpcLog>(1024);
        let url = self.ws_url.clone();

        let handle = tokio::spawn(async move {
            loop {
                save_log_to_file(&format!("[ws] connecting to {}", url));
                let (mut ws, _) = match connect_async(url.as_str()).await {
                    Ok(ok) => ok,
                    Err(e) => {
                        save_log_to_file(&format!("[ws] connect error: {e}, retrying in 3s …"));
                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                        continue;
                    }
                };
                save_log_to_file("[ws] connected");

                let sub = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "eth_subscribe",
                    "params": ["logs", filter],
                });

                save_log_to_file("[ws] sending eth_subscribe (logs)");
                if let Err(e) = ws.send(Message::Text(sub.to_string().into())).await {
                    save_log_to_file(&format!("[ws] send subscribe failed: {e}"));
                    break;
                }

                while let Some(msg) = ws.next().await {
                    match msg {
                        Ok(Message::Text(txt)) => {
                            if let Ok(v) = serde_json::from_str::<Value>(&txt) {
                                if let Some(method) = v.get("method").and_then(|m| m.as_str()) {
                                    if method == "eth_subscription" {
                                        if let Some(result) =
                                            v.get("params").and_then(|p| p.get("result"))
                                        {
                                            match serde_json::from_value::<RpcLog>(result.clone()) {
                                                Ok(log) => {
                                                    let _ = tx.send(log).await;
                                                }
                                                Err(e) => save_log_to_file(&format!(
                                                    "[ws] failed to decode log: {e}"
                                                )),
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Ok(Message::Close(frame)) => {
                            if let Some(f) = frame {
                                save_log_to_file(&format!(
                                    "[ws] CLOSE code={} reason={}",
                                    f.code, f.reason
                                ));
                            } else {
                                save_log_to_file("[ws] CLOSE (no frame)");
                            }
                            break;
                        }
                        Ok(Message::Ping(p)) => {
                            let _ = ws.send(Message::Pong(p)).await;
                        }
                        Ok(_) => {}
                        Err(e) => {
                            save_log_to_file(&format!("[ws] error: {e}"));
                            break;
                        }
                    }
                }

                save_log_to_file("[ws] stream ended, retrying in 3s …");
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
        });

        Ok((rx, handle))
    }

    /// New API – ONE WS connection, MANY `eth_subscribe` filters.
    /// We tag every filter with a string ("v2", "v3", "fm") and emit (tag, log).
    pub async fn subscribe_logs_tagged(
        &self,
        tagged_filters: Vec<(String, Filter)>,
    ) -> anyhow::Result<(mpsc::Receiver<(String, RpcLog)>, JoinHandle<()>)> {
        let url = self.ws_url.clone();
        let (tx, rx) = mpsc::channel::<(String, RpcLog)>(2048);

        let handle = tokio::spawn(async move {
            loop {
                save_log_to_file(&format!("[ws/multi] connecting to {}", url));
                let (mut ws, _) = match connect_async(url.as_str()).await {
                    Ok(ok) => ok,
                    Err(e) => {
                        save_log_to_file(&format!(
                            "[ws/multi] connect error: {e}, retrying in 3s …"
                        ));
                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                        continue;
                    }
                };
                save_log_to_file("[ws/multi] connected");

                // subscription id -> tag
                let mut sub_map: HashMap<String, String> = HashMap::new();

                // send all subscriptions
                for (i, (tag, filter)) in tagged_filters.iter().enumerate() {
                    let req_id = i + 1;
                    let sub = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": req_id,
                        "method": "eth_subscribe",
                        "params": ["logs", filter],
                    });
                    save_log_to_file(&format!("[ws/multi] sending eth_subscribe for tag={tag}"));
                    if let Err(e) = ws.send(Message::Text(sub.to_string().into())).await {
                        save_log_to_file(&format!("[ws/multi] send subscribe({tag}) failed: {e}"));
                        continue;
                    }
                }

                // wait for subscription acks
                // we’ll give it 2s to receive them
                let ack_deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);

                loop {
                    tokio::select! {
                        biased;

                        _ = tokio::time::sleep_until(ack_deadline), if !tagged_filters.is_empty() => {
                            break;
                        }

                        maybe_msg = ws.next() => {
                            let Some(msg) = maybe_msg else {
                                save_log_to_file("[ws/multi] ws closed during ack");
                                break;
                            };
                            match msg {
                                Ok(Message::Text(txt)) => {
                                    if let Ok(v) = serde_json::from_str::<Value>(&txt) {
                                        // handle {"id":1,"result":"0xsub..."}
                                        if let Some(id) = v.get("id").and_then(|i| i.as_i64()) {
                                            if let Some(sub_id) = v.get("result").and_then(|r| r.as_str()) {
                                                if let Some((tag, _)) = tagged_filters.get((id - 1) as usize) {
                                                    sub_map.insert(sub_id.to_string(), tag.clone());
                                                    save_log_to_file(&format!("[ws/multi] subscribed `{tag}` -> {sub_id}"));
                                                }
                                            }
                                            // if we've got all, break ack loop
                                            if sub_map.len() == tagged_filters.len() {
                                                break;
                                            }
                                        } else {
                                            // could also be already a log
                                            if let Some(method) = v.get("method").and_then(|m| m.as_str()) {
                                                if method == "eth_subscription" {
                                                    if let Some(params) = v.get("params") {
                                                        if let Some(sub_id) = params.get("subscription").and_then(|s| s.as_str()) {
                                                            if let Some(result) = params.get("result") {
                                                                if let Ok(log) = serde_json::from_value::<RpcLog>(result.clone()) {
                                                                    if let Some(tag) = sub_map.get(sub_id) {
                                                                        let _ = tx.send((tag.clone(), log)).await;
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                Ok(Message::Close(frame)) => {
                                    if let Some(f) = frame {
                                        save_log_to_file(&format!("[ws/multi] CLOSE code={} reason={}", f.code, f.reason));
                                    } else {
                                        save_log_to_file("[ws/multi] CLOSE (no frame)");
                                    }
                                    break;
                                }
                                Ok(Message::Ping(p)) => {
                                    let _ = ws.send(Message::Pong(p)).await;
                                }
                                Ok(_) => {}
                                Err(e) => {
                                    save_log_to_file(&format!("[ws/multi] error during ack: {e}"));
                                    break;
                                }
                            }
                        }
                    }
                }

                // now main receive loop
                while let Some(msg) = ws.next().await {
                    match msg {
                        Ok(Message::Text(txt)) => {
                            if let Ok(v) = serde_json::from_str::<Value>(&txt) {
                                if let Some(method) = v.get("method").and_then(|m| m.as_str()) {
                                    if method == "eth_subscription" {
                                        if let Some(params) = v.get("params") {
                                            if let Some(sub_id) =
                                                params.get("subscription").and_then(|s| s.as_str())
                                            {
                                                if let Some(tag) = sub_map.get(sub_id) {
                                                    if let Some(result) = params.get("result") {
                                                        match serde_json::from_value::<RpcLog>(result.clone()) {
                                                            Ok(log) => {
                                                                let _ = tx.send((tag.clone(), log)).await;
                                                            }
                                                            Err(e) => eprintln!("[ws/multi] failed to decode log: {e}"),
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Ok(Message::Close(frame)) => {
                            if let Some(f) = frame {
                                save_log_to_file(&format!(
                                    "[ws/multi] CLOSE code={} reason={}",
                                    f.code, f.reason
                                ));
                            } else {
                                save_log_to_file("[ws/multi] CLOSE (no frame)");
                            }
                            break;
                        }
                        Ok(Message::Ping(p)) => {
                            let _ = ws.send(Message::Pong(p)).await;
                        }
                        Ok(_) => {}
                        Err(e) => {
                            save_log_to_file(&format!("[ws/multi] ws error: {e}"));
                            break;
                        }
                    }
                }

                save_log_to_file("[ws/multi] stream ended, retrying in 3s …");
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
        });

        Ok((rx, handle))
    }
}
