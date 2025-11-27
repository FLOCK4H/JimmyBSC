// UNUSED
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use alloy::primitives::Address;
use super::swaps::SwapEvent;
use tokio::sync::mpsc;
use crate::libs::lookup::save_log_to_file;

/// Aggregated swap statistics per pair
#[derive(Clone, Debug, Default)]
pub struct PairSwapStats {
    pub buy_count: u32,
    pub sell_count: u32,
    pub unique_buyers: HashSet<Address>,
}

/// Central swap aggregator that collects swap events and maintains counts
#[derive(Clone)]
pub struct SwapAggregator {
    stats: Arc<RwLock<HashMap<String, PairSwapStats>>>,
}

impl SwapAggregator {
    pub fn new() -> Self {
        Self {
            stats: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get current stats for a pair
    pub async fn get_stats(&self, pair: &Address) -> (u32, u32, u32) {
        let key = format!("{:#x}", pair);
        let stats = self.stats.read().await;
        
        if let Some(pair_stats) = stats.get(&key) {
            (
                pair_stats.buy_count,
                pair_stats.sell_count,
                pair_stats.unique_buyers.len() as u32,
            )
        } else {
            (0, 0, 0)
        }
    }

    /// Process incoming swap events
    pub async fn process_event(&self, event: SwapEvent) {
        let key = format!("{:#x}", event.pair);
        let direction = if event.is_buy { "BUY" } else { "SELL" };
        
        let mut stats = self.stats.write().await;
        
        let pair_stats = stats.entry(key.clone()).or_insert_with(Default::default);
        
        if event.is_buy {
            pair_stats.buy_count += 1;
            pair_stats.unique_buyers.insert(event.trader);
        } else {
            pair_stats.sell_count += 1;
        }
        
        let new_buy = pair_stats.buy_count;
        let new_sell = pair_stats.sell_count;
        
        save_log_to_file(&format!(
            "[swap-agg] {} on {:.8}… | B:{} S:{} | trader:{:.8}…",
            direction,
            &key[..10],
            new_buy,
            new_sell,
            format!("{:#x}", event.trader)[..10].to_string()
        ));
    }

    /// Spawn a background task that processes swap events from a channel
    pub fn spawn_processor(&self, mut rx: mpsc::Receiver<SwapEvent>) {
        let aggregator = self.clone();
        
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                aggregator.process_event(event).await;
            }
            save_log_to_file("[swap-agg] Processor channel closed");
        });
    }

    /// Clean up old pairs (optional - to prevent memory growth)
    pub async fn cleanup_pair(&self, pair: &Address) {
        let key = format!("{:#x}", pair);
        let mut stats = self.stats.write().await;
        stats.remove(&key);
    }
}

