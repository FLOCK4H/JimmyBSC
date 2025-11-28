use dashmap::DashMap;
use ratatui::prelude::*;
use std::sync::Arc;

pub type ConfigStore = Arc<DashMap<String, String>>;

#[derive(Clone, Default, Debug)]
pub struct ConfigAreas {
    pub enabled_btn: Option<Rect>,
    pub dexes: Option<Vec<Rect>>,
    pub buy_amount_input: Option<Rect>,

    // extras
    pub slippage_input: Option<Rect>,
    pub max_gwei_input: Option<Rect>,
    pub max_positions_input: Option<Rect>,
    pub tp_toggle: Option<Rect>,
    pub tp_pct_input: Option<Rect>,
    pub sl_toggle: Option<Rect>,
    pub sl_pct_input: Option<Rect>,
    pub min_liq_input: Option<Rect>,
    pub min_buys_input: Option<Rect>,
    pub max_hold_pnl_toggle: Option<Rect>,
    pub max_hold_input: Option<Rect>,
    pub accepted_quotes: Option<Vec<Rect>>,
    pub wrap_ratio_input: Option<Rect>,
    pub avoid_chinese_toggle: Option<Rect>,
    pub freshness_input: Option<Rect>,
    pub min_pnl_input: Option<Rect>,
}

pub fn new_store_with_defaults() -> ConfigStore {
    let store: ConfigStore = Arc::new(DashMap::new());

    // Try to load from cache first
    if let Ok(cached) = crate::libs::cache::load_autotrade_cache() {
        if !cached.is_empty() {
            for (k, v) in cached {
                store.insert(k, v);
            }
            return store;
        }
    }

    // Default values if no cache
    store.insert("enabled".into(), "true".into());
    store.insert("dexes".into(), vec!["v2", "v3", "fm"].join(",").into());
    store.insert("buy_amount_wbnb".into(), "0.00001".into());

    // extras
    store.insert("slippage_pct".into(), "0.5".into());
    store.insert("max_gwei".into(), "1.0".into());
    store.insert("max_positions".into(), "3".into());
    store.insert("tp_enabled".into(), "false".into());
    store.insert("tp_pct".into(), "10".into());
    store.insert("sl_enabled".into(), "false".into());
    store.insert("sl_pct".into(), "5".into());
    store.insert("min_liquidity".into(), "1000".into()); // Minimum liquidity in USD
    store.insert("min_buys".into(), "3".into()); // Minimum number of buys before trading
                                                 // Max hold in seconds (0 = disabled)
    store.insert("max_hold_secs".into(), "0".into());
    // Apply PnL threshold on Max Hold (true = use -50%)
    store.insert("max_hold_pnl".into(), "true".into());
    store.insert(
        "accepted_quotes".into(),
        "BNB,CAKE,USDT,USD1,ASTER,WBNB".into(),
    );
    store.insert("wrap_ratio_pct".into(), "80".into());
    store.insert("avoid_chinese".into(), "false".into());
    store.insert("freshness_secs".into(), "30".into());
    store.insert("min_pnl_pct".into(), "100".into());
    store
}
