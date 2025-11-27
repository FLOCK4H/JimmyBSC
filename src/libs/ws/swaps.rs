// UNUSED
use anyhow::Result;
use alloy::providers::Provider;
use alloy::primitives::{keccak256, Address, U256, B256};
use alloy::rpc::types::eth::{Filter, Log};
use tokio::sync::mpsc;
use std::time::{SystemTime, UNIX_EPOCH};
use crate::libs::lookup::save_log_to_file;
use crate::libs::bsc::client::BscWsClient;

/// Represents a swap transaction (buy or sell)
#[derive(Clone, Debug)]
pub struct SwapEvent {
    pub pair: Address,
    pub trader: Address,
    pub is_buy: bool,  // true = buy base token, false = sell base token
    pub amount_in: U256,
    pub amount_out: U256,
    pub timestamp: u64,
    pub tx_hash: B256,
}

/// Aggregated swap info for a pair
#[derive(Clone, Debug, Default)]
pub struct SwapStats {
    pub buy_count: u32,
    pub sell_count: u32,
    pub unique_buyers: u32,
    pub last_buyer: Option<Address>,
}

// V2 Swap event: Swap(address indexed sender, uint amount0In, uint amount1In, uint amount0Out, uint amount1Out, address indexed to)
fn v2_swap_topic() -> B256 {
    keccak256("Swap(address,uint256,uint256,uint256,uint256,address)".as_bytes())
}

// V3 Swap event: Swap(address indexed sender, address indexed recipient, int256 amount0, int256 amount1, uint160 sqrtPriceX96, uint128 liquidity, int24 tick)
fn v3_swap_topic() -> B256 {
    keccak256("Swap(address,address,int256,int256,uint160,uint128,int24)".as_bytes())
}

/// Parse V2 swap event
fn parse_v2_swap(log: &Log, _base_token: Address, is_token0: bool) -> Option<SwapEvent> {
    let topics = log.topics();
    if topics.len() < 3 { return None; }
    
    // topics[1] = sender, topics[2] = to (recipient)
    let trader = Address::from_slice(&topics[2].as_slice()[12..32]);
    
    let data = log.data().data.as_ref();
    if data.len() < 128 { return None; }
    
    // Parse amounts: amount0In, amount1In, amount0Out, amount1Out
    let amount0_in = U256::from_be_slice(&data[0..32]);
    let amount1_in = U256::from_be_slice(&data[32..64]);
    let amount0_out = U256::from_be_slice(&data[64..96]);
    let amount1_out = U256::from_be_slice(&data[96..128]);
    
    // Determine if this is a buy or sell of the base token
    let (is_buy, amount_in, amount_out) = if is_token0 {
        // Base is token0: buying means amount0Out > 0 (getting base), selling means amount0In > 0 (giving base)
        if amount0_out > U256::ZERO {
            (true, amount1_in, amount0_out)  // Buy: pay token1, get token0
        } else {
            (false, amount0_in, amount1_out)  // Sell: pay token0, get token1
        }
    } else {
        // Base is token1: buying means amount1Out > 0, selling means amount1In > 0
        if amount1_out > U256::ZERO {
            (true, amount0_in, amount1_out)  // Buy: pay token0, get token1
        } else {
            (false, amount1_in, amount0_out)  // Sell: pay token1, get token0
        }
    };
    
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    
    Some(SwapEvent {
        pair: log.address(),
        trader,
        is_buy,
        amount_in,
        amount_out,
        timestamp,
        tx_hash: log.transaction_hash.unwrap_or_default(),
    })
}

/// Subscribe to swap events for a specific pair (V2)
pub async fn track_v2_pair_swaps(
    pair: Address,
    base_token: Address,
    is_token0: bool,
    ws: BscWsClient,
    tx: mpsc::Sender<SwapEvent>,
) {
    let swap_topic = v2_swap_topic();
    save_log_to_file(&format!("[swap-tracker] V2 Swap topic: {:#x}", swap_topic));
    
    let filter = Filter::new()
        .address(pair)
        .event_signature(swap_topic);
    
    save_log_to_file(&format!("[swap-tracker] Subscribing to V2 swaps for pair {:#x}", pair));
    
    loop {
        match ws.subscribe_logs(filter.clone()).await {
            Ok((mut rx_logs, handle)) => {
                save_log_to_file(&format!("[swap-tracker] ✓ V2 subscription active for {:#x}", pair));
                
                let mut event_count = 0u32;
                while let Some(log_item) = rx_logs.recv().await {
                    event_count += 1;
                    save_log_to_file(&format!("[swap-tracker] V2 raw log #{} received for {:#x}", event_count, pair));
                    
                    if let Some(swap) = parse_v2_swap(&log_item, base_token, is_token0) {
                        let direction = if swap.is_buy { "BUY" } else { "SELL" };
                        save_log_to_file(&format!(
                            "[swap-tracker] V2 {} from {:#x} | tx: {:#x}",
                            direction,
                            swap.trader,
                            swap.tx_hash
                        ));
                        
                        if tx.try_send(swap).is_err() {
                            save_log_to_file("[swap-tracker] ERROR: Failed to send swap to aggregator (channel full)");
                        }
                    } else {
                        save_log_to_file(&format!("[swap-tracker] V2 failed to parse log for {:#x}", pair));
                    }
                }
                
                handle.abort();
                save_log_to_file(&format!("[swap-tracker] Stream ended for {:#x}, retrying...", pair));
            }
            Err(e) => {
                save_log_to_file(&format!("[swap-tracker] Subscribe failed for {:#x}: {}", pair, e));
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
        }
    }
}

/// Parse V3 swap event
fn parse_v3_swap(log: &Log, _base_token: Address, is_token0: bool) -> Option<SwapEvent> {
    let topics = log.topics();
    if topics.len() < 3 { return None; }
    
    // topics[1] = sender, topics[2] = recipient
    let trader = Address::from_slice(&topics[2].as_slice()[12..32]);
    
    let data = log.data().data.as_ref();
    if data.len() < 160 { return None; }
    
    // Parse amounts: amount0 (int256), amount1 (int256)
    let amount0_raw = &data[0..32];
    let amount1_raw = &data[32..64];
    
    // Convert from int256 to U256 (take absolute value)
    let amount0 = U256::from_be_slice(amount0_raw);
    let amount1 = U256::from_be_slice(amount1_raw);
    
    // Check sign bit to determine direction
    let amount0_negative = amount0_raw[0] & 0x80 != 0;
    let amount1_negative = amount1_raw[0] & 0x80 != 0;
    
    // Determine if this is a buy or sell of the base token
    // In V3: negative means token going out (user receiving), positive means token going in (user paying)
    let (is_buy, amount_in, amount_out) = if is_token0 {
        // Base is token0: buying means amount0 is negative (receiving base), selling means amount0 is positive (paying base)
        if amount0_negative {
            (true, amount1, amount0)  // Buy: pay token1, get token0
        } else {
            (false, amount0, amount1)  // Sell: pay token0, get token1
        }
    } else {
        // Base is token1: buying means amount1 is negative, selling means amount1 is positive
        if amount1_negative {
            (true, amount0, amount1)  // Buy: pay token0, get token1
        } else {
            (false, amount1, amount0)  // Sell: pay token1, get token0
        }
    };
    
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    
    Some(SwapEvent {
        pair: log.address(),
        trader,
        is_buy,
        amount_in,
        amount_out,
        timestamp,
        tx_hash: log.transaction_hash.unwrap_or_default(),
    })
}

/// Subscribe to swap events for a specific pool (V3)
pub async fn track_v3_pool_swaps(
    pool: Address,
    base_token: Address,
    is_token0: bool,
    ws: BscWsClient,
    tx: mpsc::Sender<SwapEvent>,
) {
    let swap_topic = v3_swap_topic();
    save_log_to_file(&format!("[swap-tracker] V3 Swap topic: {:#x}", swap_topic));
    
    let filter = Filter::new()
        .address(pool)
        .event_signature(swap_topic);
    
    save_log_to_file(&format!("[swap-tracker] Subscribing to V3 swaps for pool {:#x}", pool));
    
    loop {
        match ws.subscribe_logs(filter.clone()).await {
            Ok((mut rx_logs, handle)) => {
                save_log_to_file(&format!("[swap-tracker] ✓ V3 subscription active for {:#x}", pool));
                
                let mut event_count = 0u32;
                while let Some(log_item) = rx_logs.recv().await {
                    event_count += 1;
                    save_log_to_file(&format!("[swap-tracker] V3 raw log #{} received for {:#x}", event_count, pool));
                    
                    if let Some(swap) = parse_v3_swap(&log_item, base_token, is_token0) {
                        let direction = if swap.is_buy { "BUY" } else { "SELL" };
                        save_log_to_file(&format!(
                            "[swap-tracker] V3 {} from {:#x} | tx: {:#x}",
                            direction,
                            swap.trader,
                            swap.tx_hash
                        ));
                        
                        if tx.try_send(swap).is_err() {
                            save_log_to_file("[swap-tracker] ERROR: Failed to send swap to aggregator (channel full)");
                        }
                    } else {
                        save_log_to_file(&format!("[swap-tracker] V3 failed to parse log for {:#x}", pool));
                    }
                }
                
                handle.abort();
                save_log_to_file(&format!("[swap-tracker] Stream ended for {:#x}, retrying...", pool));
            }
            Err(e) => {
                save_log_to_file(&format!("[swap-tracker] Subscribe failed for {:#x}: {}", pool, e));
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
        }
    }
}

