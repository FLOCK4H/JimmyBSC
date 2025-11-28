
use {
    crate::app::auto_trade::pair_key_addr,
    crate::app::pair_state::{detect_source, extract_price_f64, PairState},
    crate::libs::ws::pairs::PairInfo,
    crate::shared::should_avoid_name,
    once_cell::sync::Lazy,
    std::collections::{HashMap, HashSet, VecDeque},
    std::sync::RwLock,
    std::time::Instant,
};

static PAIR_METRICS: Lazy<RwLock<HashMap<String, (f64, Instant)>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

pub fn pair_metrics(pair_key: &str) -> Option<(f64, Instant)> {
    PAIR_METRICS
        .read()
        .ok()
        .and_then(|m| m.get(pair_key).cloned())
}

fn update_metrics(pair_key: &str, pnl_pct: Option<f64>, first_seen: Instant) {
    if let Some(pnl) = pnl_pct {
        if let Ok(mut m) = PAIR_METRICS.write() {
            m.insert(pair_key.to_string(), (pnl, first_seen));
        }
    }
}

/// Fast, synchronous updater for Hermes pair state.
/// - Only updates `pairs_map`/`pair_keys`/`sold_pairs`
/// - No trading logic, no async/RPC
pub fn update_pairs_state(
    l1: String,
    l2: String,
    pair_info: PairInfo,
    pairs_map: &mut HashMap<String, PairState>,
    pair_keys: &mut VecDeque<String>,
    _sold_pairs: &mut HashSet<String>,
    max_pairs: usize,
) {
    if l1.contains("Price: ?") {
        return;
    }
    let price_opt = extract_price_f64(&l1);
    let src = detect_source(&l1);

    // Remove if price explicitly zero
    if let Some(p) = price_opt {
        if p == 0.0 {
            let existed = pairs_map.remove(&pair_key_addr(pair_info.pair)).is_some();
            if existed {
                pair_keys.retain(|k| k != &pair_key_addr(pair_info.pair));
            }
            return;
        }
    }

    if let Some(entry) = pairs_map.get_mut(&pair_key_addr(pair_info.pair)) {
        entry.upair_address = l1;
        entry.link_line = l2.clone();
        entry.source = src;
        entry.buy_count = pair_info.buy_count;
        entry.sell_count = pair_info.sell_count;
        entry.liquidity_usd = pair_info.liquidity_usd;

        if let Some(p) = price_opt {
            entry.last_price = Some(p);
            if entry.first_price.is_none() {
                entry.first_price = Some(p);
            }
        }
        if let (Some(fp), Some(lp)) = (entry.first_price, entry.last_price) {
            if fp > 0.0 {
                let pct: f64 = (lp / fp - 1.0) * 100.0;
                let nowi = Instant::now();
                let new_pnl: i32 = (pct * 100.0).round() as i32;
                if entry.last_pnl.map(|v| v != new_pnl).unwrap_or(true) {
                    entry.last_pnl_change_at = nowi;
                }
                if pct <= -0.1 {
                    if entry.below_thresh_since.is_none() {
                        entry.below_thresh_since = Some(nowi);
                    }
                } else {
                    entry.below_thresh_since = None;
                }
                if new_pnl != 0 {
                    entry.last_nonzero_seen = nowi;
                }
                update_metrics(
                    &pair_key_addr(pair_info.pair),
                    Some(new_pnl as f64 / 100.0),
                    entry.last_nonzero_seen,
                );
            }
        }
    } else {
        // Freeze intake if MAX_PAIRS reached; only update existing entries above
        if pair_keys.len() >= max_pairs {
            return;
        }
        if should_avoid_name(&pair_info.symbol_base) || should_avoid_name(&pair_info.symbol_quote) {
            return;
        }
        let nowi = Instant::now();

        let mut st = PairState {
            upair_address: l1,
            link_line: l2.clone(),
            first_price: None,
            last_price: None,
            source: src,
            last_nonzero_seen: nowi,
            last_pnl_change_at: nowi,
            last_pnl: None,
            below_thresh_since: None,
            liquidity_usd: pair_info.liquidity_usd,
            buy_count: pair_info.buy_count,
            sell_count: pair_info.sell_count,
        };
        if let Some(p) = price_opt {
            st.first_price = Some(p);
            st.last_price = Some(p);
        }
        if let (Some(fp), Some(lp)) = (st.first_price, st.last_price) {
            if fp > 0.0 {
                let pct = (lp / fp - 1.0) * 100.0;
                let new_pnl: i32 = (pct * 100.0).round() as i32;
                st.last_pnl = Some(new_pnl);
                if pct <= -0.1 {
                    st.below_thresh_since = Some(nowi);
                }
                update_metrics(
                    &pair_key_addr(pair_info.pair),
                    Some(new_pnl as f64 / 100.0),
                    nowi,
                );
            }
        }
        pairs_map.insert(pair_key_addr(pair_info.pair), st);
        pair_keys.push_back(pair_key_addr(pair_info.pair));
    }
}
