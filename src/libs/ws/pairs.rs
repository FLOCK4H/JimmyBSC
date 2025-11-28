use anyhow::Result;

use alloy::primitives::keccak256;
use alloy::providers::Provider;
use alloy::rpc::types::eth::Filter;
use futures_util::future::join3;
use tokio::sync::mpsc;

use pancakes::pancake::pancake_swap::addresses::PANCAKE_V3_FACTORY;
use pancakes::pancake::pancake_swap_v2::addresses::PANCAKE_V2_FACTORY;
use pancakes::plug::v2::{enrich_v2_pair_created, try_parse_v2_pair_topics, v2_pair_created_topic};
use pancakes::plug::{enrich_v3_pool_created, try_parse_v3_pool_topics};

use crate::libs::bsc::client::BscWsClient;
use crate::libs::lookup::addr_to_symbol;
use crate::libs::lookup::{save_log_to_file, trim_chars};
use alloy::primitives::{Address, B256, U256};
use pancakes::pancake::pancake_swap::addresses::*;
use pancakes::pancake::pancake_swap::router::format_token as fmt_token;
use pancakes::plug::price::{get_liquidity_v2, get_liquidity_v3, get_price_v2, get_price_v3};
use pancakes::plug::v2::V2PairCreated;
use pancakes::plug::v3::V3PoolCreated;

use chrono::Local;
use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Semaphore;
// four.meme imports
use fourmeme::abi::IERC20Meta as FmErc20;
use fourmeme::abi::ITokenManagerHelper3;
use fourmeme::{TOKEN_MANAGER_2, TOKEN_MANAGER_HELPER_3};

// for file logging (save_log_to_file already imported above with trim_chars)

/// Parse liquidity from USD string format like "$1234.56" to f64
fn parse_liquidity_usd(liq_str: &str) -> Option<f64> {
    let s = liq_str.trim().trim_start_matches('$').replace(",", "");
    if s == "…" || s.is_empty() {
        return None;
    }
    s.parse::<f64>().ok()
}

/// Extract price from formatted line like "v2 | ... | Price: 0.12345"
fn extract_price_f64(line: &str) -> Option<f64> {
    let needle = "| Price:";
    let idx = line.find(needle)?;
    let after = &line[idx + needle.len()..];
    let trimmed = after.trim();
    let mut end = trimmed.len();
    for (i, ch) in trimmed.char_indices() {
        if ch.is_whitespace() || ch == '|' {
            end = i;
            break;
        }
    }
    trimmed[..end].trim().parse::<f64>().ok()
}

#[derive(Clone, Debug)]
pub struct PairInfo {
    pub addr1: Address,
    pub addr2: Address,
    pub pair: Address,
    pub fee: Option<u32>,
    pub tick_spacing: Option<i32>,
    pub symbol_base: String,
    pub symbol_quote: String,
    pub liquidity_usd: Option<f64>, // Liquidity in USD
    pub buy_count: u32,             // Real buy transactions (from swap events)
    pub sell_count: u32,            // Real sell transactions
    pub unique_buyers: u32,         // Number of unique buyer addresses
}

pub async fn pancakev2_stream(
    tx_v2: mpsc::Sender<(String, String, PairInfo)>,
    ws_v2: BscWsClient,
    provider_v2: impl Provider + Clone + 'static,
) {
    let topic0 = v2_pair_created_topic();
    let base_filter = Filter::new()
        .address(PANCAKE_V2_FACTORY)
        .event_signature(topic0);

    // bound concurrent enrichers to avoid CPU/RPC stalls blocking the receiver
    let sem = Arc::new(Semaphore::new(64));

    loop {
        let filter = base_filter.clone();
        match ws_v2.subscribe_logs(filter).await {
            Ok((mut rx_logs, handle)) => {
                save_log_to_file("[ws/v2] subscribed to PancakeV2 PairCreated");
                while let Some(log_item) = rx_logs.recv().await {
                    if let Some((t0, t1)) = try_parse_v2_pair_topics(&log_item) {
                        let permit = sem.clone().acquire_owned().await.unwrap();
                        let tx = tx_v2.clone();
                        let prov = provider_v2.clone();
                        // Parse non-indexed `pair` field from event data if present:
                        // PairCreated(address indexed token0, address indexed token1, address pair, uint)
                        // -> first 32 bytes of data contain the pair address (right-most 20 bytes)
                        let pair_from_log: Option<Address> = {
                            let data = log_item.data().data.as_ref();
                            if data.len() >= 32 {
                                let a = addr_from_word(&data[0..32]);
                                if a != Address::ZERO {
                                    Some(a)
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        };

                        tokio::spawn(async move {
                            let _p = permit;

                            if let Ok(info) = enrich_v2_pair_created(prov.clone(), t0, t1).await {
                                let (base, quote) = if info.token0 == WBNB && info.token1 != WBNB {
                                    (info.token1, WBNB)
                                } else if info.token1 == WBNB && info.token0 != WBNB {
                                    (info.token0, WBNB)
                                } else if info.token0 == USDT && info.token1 != USDT {
                                    (info.token1, USDT)
                                } else if info.token1 == USDT && info.token0 != USDT {
                                    (info.token0, USDT)
                                } else {
                                    (info.token0, info.token1)
                                };

                                let mut pair_addr = if info.pair != Address::ZERO {
                                    info.pair
                                } else if let Some(p) = pair_from_log {
                                    p
                                } else {
                                    Address::ZERO
                                };
                                if pair_addr == Address::ZERO {
                                    if let Some(addr) = v2_get_pair(
                                        prov.clone(),
                                        PANCAKE_V2_FACTORY,
                                        info.token0,
                                        info.token1,
                                    )
                                    .await
                                    {
                                        pair_addr = addr;
                                    } else {
                                        save_log_to_file(
                                            "[ws/v2] pair ZERO + factory.getPair failed; skip",
                                        );
                                        return;
                                    }
                                }

                                let joined = tokio::time::timeout(
                                    Duration::from_secs(2),
                                    join3(
                                        addr_to_symbol(prov.clone(), base),
                                        addr_to_symbol(prov.clone(), quote),
                                        get_price_v2(prov.clone(), base, quote),
                                    ),
                                )
                                .await;

                                let (s_base, s_quote, pq) = match joined {
                                    Ok(t) => t,
                                    Err(_) => (
                                        Err(anyhow::anyhow!("timeout")),
                                        Err(anyhow::anyhow!("timeout")),
                                        Err(anyhow::anyhow!("timeout")),
                                    ),
                                };

                                let sym_base = s_base.unwrap_or_else(|_| format!("{:#x}", base));
                                let sym_quote = s_quote.unwrap_or_else(|_| format!("{:#x}", quote));

                                let price = match pq {
                                    Ok(q) => fmt_token(q.amount_out_base_units, q.decimals_out),
                                    Err(_) => {
                                        match v2_price_from_reserves(
                                            prov.clone(),
                                            pair_addr,
                                            base,
                                            quote,
                                        )
                                        .await
                                        {
                                            Some(s) => s,
                                            None => "?".into(),
                                        }
                                    }
                                };

                                let (liq_q_units, dec_q) =
                                    match get_liquidity_v2(prov.clone(), pair_addr, quote).await {
                                        Ok(t) => t,
                                        Err(_) => (U256::ZERO, 18u32),
                                    };
                                let liq_usd =
                                    quote_units_to_usd(prov.clone(), quote, liq_q_units, dec_q)
                                        .await;
                                let liq_s_raw = liq_usd.clone().unwrap_or_else(|| "…".to_string());

                                let sym_base_s = trim_chars(&sym_base, 8);
                                let sym_quote_s = trim_chars(&sym_quote, 8);
                                let liq_s = trim_chars(&liq_s_raw, 13);
                                let price_s = trim_chars(&price, 13);

                                let line1 = format!(
                                    "v2 | Base: {} <-> Quote: {} | Liq: {} | Price: {}",
                                    sym_base_s, sym_quote_s, liq_s, price_s
                                );
                                let link = format!(
                                    "{} https://dexscreener.com/bsc/{}",
                                    Local::now().format("%H:%M"),
                                    format!("{:#x}", pair_addr)
                                );

                                let pair_info = PairInfo {
                                    addr1: info.token0,
                                    addr2: info.token1,
                                    pair: pair_addr,
                                    fee: None,
                                    tick_spacing: None,
                                    symbol_base: sym_base.clone(),
                                    symbol_quote: sym_quote.clone(),
                                    liquidity_usd: parse_liquidity_usd(&liq_s_raw),
                                    buy_count: 0,
                                    sell_count: 0,
                                    unique_buyers: 0,
                                };

                                // initial delivery must not drop
                                let _ = tx
                                    .send((line1.clone(), link.clone(), pair_info.clone()))
                                    .await;

                                // refresh ticker stays best-effort
                                let prov_r = prov.clone();
                                let tx_r = tx.clone();
                                tokio::spawn(async move {
                                    let mut ticker = tokio::time::interval(Duration::from_secs(3));
                                    let mut buy_count = 0u32;
                                    let mut sell_count = 0u32;
                                    let mut last_price: Option<f64> = None;

                                    loop {
                                        ticker.tick().await;

                                        let (liq_q_units, dec_q) = match get_liquidity_v2(
                                            prov_r.clone(),
                                            pair_addr,
                                            quote,
                                        )
                                        .await
                                        {
                                            Ok(t) => t,
                                            Err(_) => (U256::ZERO, 18u32),
                                        };
                                        let liq_usd_r = quote_units_to_usd(
                                            prov_r.clone(),
                                            quote,
                                            liq_q_units,
                                            dec_q,
                                        )
                                        .await;
                                        let liq_s_r = trim_chars(
                                            &liq_usd_r.clone().unwrap_or_else(|| "…".to_string()),
                                            13,
                                        );

                                        let price_r =
                                            match get_price_v2(prov_r.clone(), base, quote).await {
                                                Ok(q) => {
                                                    let s = fmt_token(
                                                        q.amount_out_base_units,
                                                        q.decimals_out,
                                                    );
                                                    let raw = q
                                                        .amount_out_base_units
                                                        .to_string()
                                                        .parse::<f64>()
                                                        .unwrap_or(0.0);
                                                    let divisor = 10f64.powi(q.decimals_out as i32);
                                                    let current = raw / divisor;
                                                    if let Some(prev) = last_price {
                                                        if current > prev {
                                                            buy_count += 1;
                                                        } else if current < prev {
                                                            sell_count += 1;
                                                        }
                                                    }
                                                    last_price = Some(current);
                                                    s
                                                }
                                                Err(_) => price.clone(),
                                            };

                                        let line1_r = format!(
                                            "v2 | Base: {} <-> Quote: {} | Liq: {} | Price: {}",
                                            sym_base_s,
                                            sym_quote_s,
                                            liq_s_r,
                                            trim_chars(&price_r, 13)
                                        );
                                        let pair_info_r = PairInfo {
                                            addr1: info.token0,
                                            addr2: info.token1,
                                            pair: pair_addr,
                                            fee: None,
                                            tick_spacing: None,
                                            symbol_base: sym_base.clone(),
                                            symbol_quote: sym_quote.clone(),
                                            liquidity_usd: liq_usd_r
                                                .and_then(|s| parse_liquidity_usd(&s)),
                                            buy_count,
                                            sell_count,
                                            unique_buyers: 0,
                                        };
                                        let _ = tx_r.try_send((line1_r, link.clone(), pair_info_r));
                                    }
                                });
                            }
                        });
                    }
                }
                handle.abort();
                save_log_to_file("[ws/v2] stream ended, retrying in 3s …");
            }
            Err(e) => {
                save_log_to_file(&format!("[ws/v2] subscribe_logs failed: {}", e));
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        }
    }
}

pub async fn pancakev3_stream(
    tx_v3: mpsc::Sender<(String, String, PairInfo)>,
    ws_v3: BscWsClient,
    provider_v3: impl Provider + Clone + 'static,
) {
    let topic0 = keccak256("PoolCreated(address,address,uint24,int24,address)".as_bytes());
    let base_filter = Filter::new()
        .address(PANCAKE_V3_FACTORY)
        .event_signature(topic0);

    use std::sync::Arc;
    use tokio::sync::Semaphore;
    let sem = Arc::new(Semaphore::new(64)); // limit concurrent enrichers

    loop {
        let filter = base_filter.clone();
        match ws_v3.subscribe_logs(filter).await {
            Ok((mut rx_logs, handle)) => {
                save_log_to_file("[ws/v3] subscribed to PancakeV3 PoolCreated");
                while let Some(log_item) = rx_logs.recv().await {
                    if let Some((t0, t1)) = try_parse_v3_pool_topics(&log_item) {
                        let permit = sem.clone().acquire_owned().await.unwrap();
                        let tx = tx_v3.clone();
                        let prov = provider_v3.clone();

                        tokio::spawn(async move {
                            let _p = permit;

                            if let Ok(info) = enrich_v3_pool_created(prov.clone(), t0, t1).await {
                                let (base, quote) = if info.token0 == WBNB && info.token1 != WBNB {
                                    (info.token1, WBNB)
                                } else if info.token1 == WBNB && info.token0 != WBNB {
                                    (info.token0, WBNB)
                                } else if info.token0 == USDT && info.token1 != USDT {
                                    (info.token1, USDT)
                                } else if info.token1 == USDT && info.token0 != USDT {
                                    (info.token0, USDT)
                                } else {
                                    (info.token0, info.token1)
                                };

                                let joined = tokio::time::timeout(
                                    std::time::Duration::from_secs(2),
                                    join3(
                                        addr_to_symbol(prov.clone(), base),
                                        addr_to_symbol(prov.clone(), quote),
                                        get_price_v3(prov.clone(), base, quote, Some(info.fee)),
                                    ),
                                )
                                .await;

                                let (s_base, s_quote, pq) = match joined {
                                    Ok(t) => t,
                                    Err(_) => (
                                        Err(anyhow::anyhow!("timeout")),
                                        Err(anyhow::anyhow!("timeout")),
                                        Err(anyhow::anyhow!("timeout")),
                                    ),
                                };

                                let sym_base = s_base.unwrap_or_else(|_| format!("{:#x}", base));
                                let sym_quote = s_quote.unwrap_or_else(|_| format!("{:#x}", quote));
                                let price = match pq {
                                    Ok(q) => fmt_token(q.amount_out_base_units, q.decimals_out),
                                    Err(_) => "?".into(),
                                };

                                // Resolve pool address robustly (fallback to factory.getPool if zero)
                                let mut pool_addr = info.pool;
                                if pool_addr == Address::ZERO {
                                    if let Some(addr) = v3_get_pool(
                                        prov.clone(),
                                        PANCAKE_V3_FACTORY,
                                        base,
                                        quote,
                                        info.fee,
                                    )
                                    .await
                                    {
                                        pool_addr = addr;
                                    } else {
                                        save_log_to_file(
                                            "[ws/v3] pool ZERO + factory.getPool failed; skip",
                                        );
                                        return;
                                    }
                                }

                                let (liq_q_units, dec_q) =
                                    match get_liquidity_v3(prov.clone(), pool_addr, quote).await {
                                        Ok(t) => t,
                                        Err(_) => (U256::ZERO, 18u32),
                                    };
                                let liq_usd =
                                    quote_units_to_usd(prov.clone(), quote, liq_q_units, dec_q)
                                        .await;
                                let liq_s_raw = liq_usd.unwrap_or_else(|| "…".to_string());
                                let liq_s = trim_chars(&liq_s_raw, 13);

                                let line1 = format!(
                                    "v3 | Base: {} <-> Quote: {} | Liq: {} | Price: {}",
                                    sym_base, sym_quote, liq_s, price
                                );
                                let link = format!(
                                    "{} https://dexscreener.com/bsc/{}",
                                    Local::now().format("%H:%M"),
                                    format!("{:#x}", pool_addr)
                                );

                                let liquidity_usd = {
                                    let s = &liq_s_raw;
                                    let s2 = s.trim().trim_start_matches('$').replace(",", "");
                                    s2.parse::<f64>().ok()
                                };

                                let pair_info = PairInfo {
                                    addr1: info.token0,
                                    addr2: info.token1,
                                    pair: pool_addr,
                                    fee: Some(info.fee),
                                    tick_spacing: Some(info.tick_spacing),
                                    symbol_base: sym_base.clone(),
                                    symbol_quote: sym_quote.clone(),
                                    liquidity_usd,
                                    buy_count: 0,
                                    sell_count: 0,
                                    unique_buyers: 0,
                                };

                                // FIRST publish must never drop
                                let _ = tx
                                    .send((line1.clone(), link.clone(), pair_info.clone()))
                                    .await;

                                // lightweight refresh loop (best-effort)
                                let prov_r = prov.clone();
                                let tx_r = tx.clone();
                                let sym_base_c = sym_base.clone();
                                let sym_quote_c = sym_quote.clone();
                                let fee = info.fee;
                                let tick_spacing = info.tick_spacing;
                                let pool_addr_captured = pool_addr;
                                let initial_price = extract_price_f64(&line1);

                                tokio::spawn(async move {
                                    let mut ticker = tokio::time::interval(Duration::from_secs(3));
                                    let mut last_price = initial_price;
                                    let mut buy_count = 0u32;
                                    let mut sell_count = 0u32;

                                    loop {
                                        ticker.tick().await;
                                        if let Ok(q) =
                                            get_price_v3(prov_r.clone(), base, quote, Some(fee))
                                                .await
                                        {
                                            let price_refresh =
                                                fmt_token(q.amount_out_base_units, q.decimals_out);
                                            let raw = q
                                                .amount_out_base_units
                                                .to_string()
                                                .parse::<f64>()
                                                .unwrap_or(0.0);
                                            let divisor = 10f64.powi(q.decimals_out as i32);
                                            let current_price = raw / divisor;
                                            if current_price > 0.0 {
                                                if let Some(prev) = last_price {
                                                    if current_price > prev * 1.001 {
                                                        buy_count += 1;
                                                    } else if current_price < prev * 0.999 {
                                                        sell_count += 1;
                                                    }
                                                }
                                                last_price = Some(current_price);
                                            }

                                            let (liq_q_units, dec_q) = match get_liquidity_v3(
                                                prov_r.clone(),
                                                pool_addr_captured,
                                                quote,
                                            )
                                            .await
                                            {
                                                Ok(t) => t,
                                                Err(_) => (U256::ZERO, 18u32),
                                            };
                                            let liq_usd_refresh = quote_units_to_usd(
                                                prov_r.clone(),
                                                quote,
                                                liq_q_units,
                                                dec_q,
                                            )
                                            .await;
                                            let liq_s_raw_refresh =
                                                liq_usd_refresh.unwrap_or_else(|| "…".to_string());
                                            let liq_s_refresh = trim_chars(&liq_s_raw_refresh, 13);
                                            let liquidity_usd_refresh = {
                                                let s = liq_s_raw_refresh
                                                    .trim()
                                                    .trim_start_matches('$')
                                                    .replace(",", "");
                                                s.parse::<f64>().ok()
                                            };

                                            let line1_refresh = format!(
                                                "v3 | Base: {} <-> Quote: {} | Liq: {} | Price: {}",
                                                trim_chars(&sym_base_c, 8),
                                                trim_chars(&sym_quote_c, 8),
                                                liq_s_refresh,
                                                price_refresh
                                            );
                                            let pair_info_refresh = PairInfo {
                                                addr1: base,
                                                addr2: quote,
                                                pair: pool_addr_captured,
                                                fee: Some(fee),
                                                tick_spacing: Some(tick_spacing),
                                                symbol_base: sym_base_c.clone(),
                                                symbol_quote: sym_quote_c.clone(),
                                                liquidity_usd: liquidity_usd_refresh,
                                                buy_count,
                                                sell_count,
                                                unique_buyers: 0,
                                            };
                                            let _ = tx_r.try_send((
                                                line1_refresh,
                                                link.clone(),
                                                pair_info_refresh,
                                            ));
                                        }
                                    }
                                });
                            }
                        });
                    }
                }
                handle.abort();
                save_log_to_file("[ws/v3] stream ended, retrying in 3s …");
            }
            Err(e) => {
                save_log_to_file(&format!("[ws/v3] subscribe_logs failed: {}", e));
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        }
    }
}

use alloy::primitives::b256;

const TM2_TOPIC_TOKEN_CREATE: B256 =
    b256!("0x0a5575b3648bae2210cee56bf33254cc1ddfbc7bf637c0af2ac18b14fb1bae19");
// Some launches show this around the same time; token address is in data, but not fixed offset.
const TM2_TOPIC_ALT_CREATE: B256 =
    b256!("0x7db52723a3b2cdd6164364b3b766e65e540d7be48ffa89582956d8eaebe62942");

// How “new” is allowed (seconds). Tune if needed.
const NEW_WINDOW_SECS: u64 = 120;

// Read a 20-byte address out of a 32-byte word (last 20 bytes)
#[inline]
fn addr_from_word(word32: &[u8]) -> Address {
    Address::from_slice(&word32[12..32])
}

pub async fn fourmeme_stream(
    tx: mpsc::Sender<(String, String, PairInfo)>,
    ws_fm: BscWsClient,
    provider_fm: impl Provider + Clone + 'static,
) {
    // Keep it simple: filter by address, then gate by topic0 in code.
    let base_filter = Filter::new().address(TOKEN_MANAGER_2);
    let mut seen_bases: HashSet<Address> = HashSet::new();

    loop {
        let filter = base_filter.clone();
        match ws_fm.subscribe_logs(filter).await {
            Ok((mut rx_logs, handle)) => {
                save_log_to_file("[ws/fm] subscribed to TOKEN_MANAGER_2 (creation-filtered)");

                while let Some(log_item) = rx_logs.recv().await {
                    let topics = log_item.topics();
                    if topics.is_empty() {
                        continue;
                    }
                    let topic0 = topics[0];

                    // Only handle the two creation-ish topics we identified
                    if topic0 != TM2_TOPIC_TOKEN_CREATE && topic0 != TM2_TOPIC_ALT_CREATE {
                        continue;
                    }

                    let data = log_item.data().data.as_ref();
                    if data.len() < 32 {
                        continue;
                    }

                    // 1) Extract candidate token(s)
                    let mut candidates: Vec<Address> = Vec::new();

                    if topic0 == TM2_TOPIC_TOKEN_CREATE {
                        // In your logs, token is the FIRST word for 0x0a5575…
                        let token = addr_from_word(&data[0..32]);
                        if token != Address::ZERO {
                            candidates.push(token);
                        }
                    } else {
                        // For 0x7db527… we’ve seen token both as word0 and elsewhere.
                        // Scan first 8 words; pick addrs ending with 0x…4444 (fourmeme pattern),
                        // and also just try all unique-looking addresses.
                        for w in data.chunks(32).take(8) {
                            if w.len() != 32 {
                                break;
                            }
                            let a = addr_from_word(w);
                            if a != Address::ZERO && !candidates.contains(&a) {
                                // optional heuristic: many four.meme tokens end with 0x….4444
                                // keep both the heuristic hit and any others; helper call will verify.
                                candidates.push(a);
                            }
                        }
                    }

                    if candidates.is_empty() {
                        continue;
                    }

                    // 2) For each candidate, verify via helper AND enforce “newness”
                    let helper =
                        ITokenManagerHelper3::new(TOKEN_MANAGER_HELPER_3, provider_fm.clone());
                    let now_secs = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();

                    'cand: for base in candidates {
                        if seen_bases.contains(&base) {
                            continue;
                        }

                        let info: ITokenManagerHelper3::getTokenInfoReturn =
                            match tokio::time::timeout(
                                Duration::from_millis(900),
                                helper.getTokenInfo(base).call(),
                            )
                            .await
                            {
                                Ok(Ok(ret)) => ret,
                                _ => continue,
                            };

                        // Must be a real four.meme token
                        if info.tokenManager == Address::ZERO {
                            continue;
                        }

                        // Enforce freshness: launchTime within NEW_WINDOW_SECS of now.
                        // (getTokenInfo returns chain launchTime; good enough for filtering)
                        let launch = info.launchTime;
                        let launch_u64: u64 = launch.try_into().unwrap_or(u64::MAX);
                        if launch_u64 == 0 {
                            continue;
                        }
                        if now_secs.saturating_sub(launch_u64) > NEW_WINDOW_SECS {
                            // Older token referenced by other events -> ignore
                            continue;
                        }

                        // Resolve quote & symbols
                        let quote = if info.quote == Address::ZERO {
                            Address::ZERO
                        } else {
                            info.quote
                        };

                        let sym_base = match addr_to_symbol(provider_fm.clone(), base).await {
                            Ok(s) => s,
                            Err(_) => format!("{base:#x}"),
                        };
                        let sym_quote = if quote == Address::ZERO {
                            "WBNB".to_string()
                        } else {
                            match addr_to_symbol(provider_fm.clone(), quote).await {
                                Ok(s) => s,
                                Err(_) => format!("{quote:#x}"),
                            }
                        };

                        let decimals_q: u32 = if quote == Address::ZERO {
                            18
                        } else {
                            FmErc20::new(quote, provider_fm.clone())
                                .decimals()
                                .call()
                                .await
                                .unwrap_or(18) as u32
                        };
                        let price_str = fourmeme::price::format_units(info.lastPrice, decimals_q);

                        // Liquidity (quote-side) in USD
                        let liq_q_units: U256 = U256::from(info.funds);
                        let liq_usd =
                            quote_units_to_usd(provider_fm.clone(), quote, liq_q_units, decimals_q)
                                .await;
                        let liq_s = liq_usd.unwrap_or_else(|| "…".to_string());

                        let link = format!(
                            "Created at: {} https://four.meme/token/{:#x}",
                            Local::now().format("%H:%M:%S"),
                            base
                        );
                        let line1 = format!(
                            "fm | Base: {} <-> Quote: {} | Liq: {} | Price: {}",
                            trim_chars(&sym_base, 8),
                            trim_chars(&sym_quote, 8),
                            trim_chars(&liq_s, 10),
                            trim_chars(&price_str, 13)
                        );

                        let liquidity_usd = parse_liquidity_usd(&liq_s);
                        let pair_info = PairInfo {
                            addr1: base,
                            addr2: quote,
                            pair: base, // no on-chain AMM pair addr here; key by base
                            fee: None,
                            tick_spacing: None,
                            symbol_base: sym_base.clone(),
                            symbol_quote: sym_quote.clone(),
                            liquidity_usd,
                            buy_count: 0,
                            sell_count: 0,
                            unique_buyers: 0,
                        };

                        let _ = tx.try_send((line1.clone(), link.clone(), pair_info.clone()));
                        save_log_to_file(&format!("[ws/fm] NEW (accepted): {} {}", line1, link));

                        // Periodically refresh fm price using TokenManagerHelper, similar to v2/v3 tickers
                        let provider_refresh = provider_fm.clone();
                        let tx_refresh = tx.clone();
                        let sym_base_c = sym_base.clone();
                        let sym_quote_c = sym_quote.clone();
                        let link_c = link.clone();
                        let initial_price = extract_price_f64(&line1);
                        tokio::spawn(async move {
                            let helper_r = ITokenManagerHelper3::new(
                                TOKEN_MANAGER_HELPER_3,
                                provider_refresh.clone(),
                            );
                            let mut ticker = tokio::time::interval(Duration::from_secs(3));
                            let mut last_price = initial_price;
                            let mut buy_count = 0u32;
                            let mut sell_count = 0u32;

                            loop {
                                ticker.tick().await;
                                if let Ok(info_r) = helper_r.getTokenInfo(base).call().await {
                                    let price_str_r =
                                        fourmeme::price::format_units(info_r.lastPrice, decimals_q);

                                    // Track buy/sell based on price movement for fourmeme
                                    let raw =
                                        info_r.lastPrice.to_string().parse::<f64>().unwrap_or(0.0);
                                    let divisor = 10f64.powi(decimals_q as i32);
                                    let current_price = raw / divisor;

                                    if current_price > 0.0 {
                                        if let Some(prev) = last_price {
                                            if current_price > prev * 1.000000000001 {
                                                buy_count += 1;
                                            } else if current_price < prev * 0.999999999999 {
                                                sell_count += 1;
                                            }
                                        }
                                        last_price = Some(current_price);
                                    }

                                    // Refresh liquidity
                                    let liq_q_units_r: U256 = U256::from(info_r.funds);
                                    let liq_usd_r = quote_units_to_usd(
                                        provider_refresh.clone(),
                                        quote,
                                        liq_q_units_r,
                                        decimals_q,
                                    )
                                    .await;
                                    let liq_s_r = liq_usd_r.unwrap_or_else(|| "…".to_string());
                                    let liquidity_usd_r = parse_liquidity_usd(&liq_s_r);

                                    let line1_refresh = format!(
                                        "fm | Base: {} <-> Quote: {} | Liq: {} | Price: {}",
                                        sym_base_c,
                                        sym_quote_c,
                                        trim_chars(&liq_s_r, 10),
                                        trim_chars(&price_str_r, 13)
                                    );
                                    let pair_info_refresh = PairInfo {
                                        addr1: base,
                                        addr2: quote,
                                        pair: base,
                                        fee: None,
                                        tick_spacing: None,
                                        symbol_base: sym_base_c.clone(),
                                        symbol_quote: sym_quote_c.clone(),
                                        liquidity_usd: liquidity_usd_r,
                                        buy_count,
                                        sell_count,
                                        unique_buyers: 0,
                                    };
                                    let _ = tx_refresh.try_send((
                                        line1_refresh,
                                        link_c.clone(),
                                        pair_info_refresh,
                                    ));
                                }
                            }
                        });

                        seen_bases.insert(base);
                        break 'cand;
                    }
                }

                handle.abort();
                save_log_to_file("[ws/fm] stream ended, retrying in 3s …");
            }
            Err(e) => {
                save_log_to_file(&format!("[ws/fm] subscribe_logs failed: {}", e));
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        }
    }
}

pub fn spawn_pair_streams<P: Provider + Clone + Send + Sync + 'static>(
    provider: P,
    ws: BscWsClient,
) -> Result<(
    mpsc::Receiver<(String, String, PairInfo)>,
    tokio::task::JoinHandle<()>,
)> {
    // was 1024 — bursts from refreshers could fill it before the TUI consumes
    let (tx, rx) = mpsc::channel::<(String, String, PairInfo)>(4096);

    let handle = tokio::spawn(async move {
        let tx_v2 = tx.clone();
        let tx_v3 = tx.clone();
        let tx_fm = tx.clone();

        let ws_v2 = ws.clone();
        let ws_v3 = ws.clone();
        let ws_fm = ws.clone();
        let provider_v2 = provider.clone();
        let provider_v3 = provider.clone();
        let provider_fm = provider.clone();

        let v2_task = tokio::spawn(async move {
            pancakev2_stream(tx_v2.clone(), ws_v2, provider_v2).await;
        });

        let v3_task = tokio::spawn(async move {
            pancakev3_stream(tx_v3.clone(), ws_v3, provider_v3).await;
        });

        let fm_task = tokio::spawn(async move {
            fourmeme_stream(tx_fm.clone(), ws_fm, provider_fm).await;
        });

        // Lightweight heartbeat: send a no-op (Price: ?) line periodically so the
        // consumer sees activity even when there are no create events for a while.
        // This is ignored by update_pairs_state and auto_trade, but confirms liveness.
        let tx_hb = tx.clone();
        let _hb_task = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(15));
            loop {
                ticker.tick().await;
                let line1 = "hb | Hermes heartbeat | Price: ?".to_string();
                let link = String::new();
                let pair_info = PairInfo {
                    addr1: Address::ZERO,
                    addr2: Address::ZERO,
                    pair: Address::ZERO,
                    fee: None,
                    tick_spacing: None,
                    symbol_base: "HB".to_string(),
                    symbol_quote: "HB".to_string(),
                    liquidity_usd: None,
                    buy_count: 0,
                    sell_count: 0,
                    unique_buyers: 0,
                };
                let _ = tx_hb.try_send((line1, link, pair_info));
            }
        });

        let _ = tokio::join!(v2_task, v3_task, fm_task);
    });

    Ok((rx, handle))
}

alloy::sol! {
    #[sol(rpc)]
    interface IPancakePairView {
        function token0() view returns (address);
        function token1() view returns (address);
        function getReserves() view returns (uint112 reserve0, uint112 reserve1, uint32 blockTimestampLast);
    }
    #[sol(rpc)]
    interface IERC20Dec {
        function decimals() view returns (uint8);
    }
    #[sol(rpc)]
    interface IPancakeV2Factory {
        function getPair(address tokenA, address tokenB) view returns (address pair);
    }
    #[sol(rpc)]
    interface IPancakeV3Factory {
        function getPool(address tokenA, address tokenB, uint24 fee) view returns (address pool);
    }
}

async fn v2_price_from_reserves<P: Provider + Clone>(
    provider: P,
    pair: Address,
    base: Address,
    quote: Address,
) -> Option<String> {
    let pairc = IPancakePairView::new(pair, provider.clone());
    let (t0, t1) = (
        pairc.token0().call().await.ok()?,
        pairc.token1().call().await.ok()?,
    );
    let reserves = pairc.getReserves().call().await.ok()?;
    let (r0, r1) = (U256::from(reserves.reserve0), U256::from(reserves.reserve1));
    if r0.is_zero() || r1.is_zero() {
        return None;
    }

    let dec_q = IERC20Dec::new(quote, provider.clone())
        .decimals()
        .call()
        .await
        .ok()?;
    let scale_q = U256::from(10u64).pow(U256::from(dec_q));

    let price_units = if base == t0 && quote == t1 {
        r1 * scale_q / r0
    } else if base == t1 && quote == t0 {
        r0 * scale_q / r1
    } else {
        return None;
    };

    Some(fmt_token(price_units, dec_q.into()))
}

async fn v2_get_pair<P: Provider + Clone>(
    provider: P,
    factory: Address,
    token_a: Address,
    token_b: Address,
) -> Option<Address> {
    let f = IPancakeV2Factory::new(factory, provider.clone());
    // Try both orders to be safe across factory implementations
    if let Ok(addr) = f.getPair(token_a, token_b).call().await {
        if addr != Address::ZERO {
            return Some(addr);
        }
    }
    if let Ok(addr2) = IPancakeV2Factory::new(factory, provider)
        .getPair(token_b, token_a)
        .call()
        .await
    {
        if addr2 != Address::ZERO {
            return Some(addr2);
        }
    }
    None
}

async fn v3_get_pool<P: Provider + Clone>(
    provider: P,
    factory: Address,
    token_a: Address,
    token_b: Address,
    fee: u32,
) -> Option<Address> {
    let f = IPancakeV3Factory::new(factory, provider.clone());
    // Try both orders to guard against implementations that require sorting
    if let Ok(addr) = f
        .getPool(token_a, token_b, alloy::primitives::aliases::U24::from(fee))
        .call()
        .await
    {
        if addr != Address::ZERO {
            return Some(addr);
        }
    }
    if let Ok(addr2) = IPancakeV3Factory::new(factory, provider)
        .getPool(token_b, token_a, alloy::primitives::aliases::U24::from(fee))
        .call()
        .await
    {
        if addr2 != Address::ZERO {
            return Some(addr2);
        }
    }
    None
}

// Convert a quote token amount (base units) into USD (USDT base units), formatted as $... string.
async fn quote_units_to_usd<P: Provider + Clone>(
    provider: P,
    quote: Address,
    amount_quote_base_units: U256,
    decimals_quote: u32,
) -> Option<String> {
    use pancakes::pancake::pancake_swap::addresses::{USDT, WBNB};
    if amount_quote_base_units.is_zero() {
        return None;
    }

    // Helper to compute 10^decimals as U256
    fn pow10(dec: u32) -> U256 {
        U256::from(10u64).pow(U256::from(dec))
    }

    // Resolve sentinel: in four.meme streams, quote == Address::ZERO means WBNB
    let quote_resolved = if quote == Address::ZERO { WBNB } else { quote };

    if quote_resolved == USDT {
        // Already USDT units
        let dec_out = match IERC20Dec::new(USDT, provider.clone())
            .decimals()
            .call()
            .await
            .ok()
        {
            Some(d) => d as u32,
            None => 18u32,
        };
        let s = fmt_token(amount_quote_base_units, dec_out.into());
        return Some(format!("${}", s));
    }

    // Prefer v2 route quote->USDT, fallback to v3 auto-detect
    let price = match get_price_v2(provider.clone(), quote_resolved, USDT).await {
        Ok(q) => Some(q),
        Err(_) => get_price_v3(provider.clone(), quote_resolved, USDT, None)
            .await
            .ok(),
    }?;

    let scale_q = pow10(decimals_quote);
    let usdt_units = amount_quote_base_units.saturating_mul(price.amount_out_base_units) / scale_q;
    let s = fmt_token(usdt_units, price.decimals_out);
    Some(format!("${}", s))
}
