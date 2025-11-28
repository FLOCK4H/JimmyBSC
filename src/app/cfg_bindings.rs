use {
    crate::libs::cache::save_autotrade_cache,
    crate::libs::sim::SimEngine,
    crate::libs::tui::{ConfigAreas, ConfigStore},
    ratatui::prelude::*,
};

pub fn cfg_bindings(
    config_areas: &ConfigAreas,
    config_store: &ConfigStore,
    sim_engine: &mut SimEngine,
    mx: u16,
    my: u16,
    focused_field: &mut Option<String>,
    input_buffer: &mut String,
) {
    let contains = |r: Option<Rect>| -> bool {
        if let Some(rr) = r {
            mx >= rr.x && mx < rr.x + rr.width && my >= rr.y && my < rr.y + rr.height
        } else {
            false
        }
    };
    let toggle_key = |store: &ConfigStore, k: &str| {
        let v = store
            .get(k)
            .map(|v| v.to_string())
            .unwrap_or_else(|| "false".into());
        let newv = if v == "true" { "false" } else { "true" };
        store.insert(k.to_string(), newv.to_string());
    };

    if contains(config_areas.enabled_btn) {
        toggle_key(config_store, "enabled");
        let _ = save_autotrade_cache(config_store);
    }
    if let Some(rects) = config_areas.dexes.as_ref() {
        for (i, r) in rects.iter().enumerate() {
            if contains(Some(*r)) {
                let label = match i {
                    0 => "v2",
                    1 => "v3",
                    2 => "fm",
                    _ => continue,
                };
                let current = config_store
                    .get("dexes")
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "v2,v3,fm".to_string());
                let mut parts: Vec<String> = current
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if let Some(pos) = parts.iter().position(|s| s == label) {
                    parts.remove(pos);
                } else {
                    parts.push(label.to_string());
                }
                // Keep deterministic order
                let mut ordered: Vec<String> = Vec::new();
                for k in ["v2", "v3", "fm"] {
                    if parts.iter().any(|s| s == k) {
                        ordered.push(k.to_string());
                    }
                }
                config_store.insert("dexes".into(), ordered.join(","));
                let _ = save_autotrade_cache(config_store);
            }
        }
    }
    if contains(config_areas.buy_amount_input) {
        *focused_field = Some("buy_amount_wbnb".to_string());
        *input_buffer = config_store
            .get("buy_amount_wbnb")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "0.00001".to_string());
    }
    if contains(config_areas.tp_toggle) {
        toggle_key(config_store, "tp_enabled");
        let _ = save_autotrade_cache(config_store);
    }
    if contains(config_areas.max_hold_pnl_toggle) {
        toggle_key(config_store, "max_hold_pnl");
        let enabled_now = config_store
            .get("max_hold_pnl")
            .map(|v| v.as_str() == "true")
            .unwrap_or(true);
        sim_engine.set_max_hold_pnl_enabled(enabled_now);
        let _ = save_autotrade_cache(config_store);
    }
    if contains(config_areas.sl_toggle) {
        toggle_key(config_store, "sl_enabled");
        let _ = save_autotrade_cache(config_store);
    }
    if contains(config_areas.slippage_input) {
        *focused_field = Some("slippage_pct".to_string());
        *input_buffer = config_store
            .get("slippage_pct")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "0.5".to_string());
    }
    if contains(config_areas.max_gwei_input) {
        *focused_field = Some("max_gwei".to_string());
        *input_buffer = config_store
            .get("max_gwei")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "1.0".to_string());
    }
    if contains(config_areas.max_positions_input) {
        *focused_field = Some("max_positions".to_string());
        *input_buffer = config_store
            .get("max_positions")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "3".to_string());
    }
    if contains(config_areas.min_liq_input) {
        *focused_field = Some("min_liquidity".to_string());
        *input_buffer = config_store
            .get("min_liquidity")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "1000".to_string());
    }
    if contains(config_areas.min_buys_input) {
        *focused_field = Some("min_buys".to_string());
        *input_buffer = config_store
            .get("min_buys")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "3".to_string());
    }
    if contains(config_areas.max_hold_input) {
        *focused_field = Some("max_hold_secs".to_string());
        *input_buffer = config_store
            .get("max_hold_secs")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "0".to_string());
    }
    if contains(config_areas.tp_pct_input) {
        *focused_field = Some("tp_pct".to_string());
        *input_buffer = config_store
            .get("tp_pct")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "10".to_string());
    }
    if contains(config_areas.sl_pct_input) {
        *focused_field = Some("sl_pct".to_string());
        *input_buffer = config_store
            .get("sl_pct")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "5".to_string());
    }
    if contains(config_areas.avoid_chinese_toggle) {
        toggle_key(config_store, "avoid_chinese");
        let _ = save_autotrade_cache(config_store);
    }
    if contains(config_areas.freshness_input) {
        *focused_field = Some("freshness_secs".to_string());
        *input_buffer = config_store
            .get("freshness_secs")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "30".to_string());
    }
    if contains(config_areas.min_pnl_input) {
        *focused_field = Some("min_pnl_pct".to_string());
        *input_buffer = config_store
            .get("min_pnl_pct")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "100".to_string());
    }
    if contains(config_areas.wrap_ratio_input) {
        *focused_field = Some("wrap_ratio_pct".to_string());
        *input_buffer = config_store
            .get("wrap_ratio_pct")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "80".to_string());
    }
    if let Some(rects) = config_areas.accepted_quotes.as_ref() {
        for (i, r) in rects.iter().enumerate() {
            if contains(Some(*r)) {
                let label = match i {
                    0 => "BNB",
                    1 => "CAKE",
                    2 => "USDT",
                    3 => "USD1",
                    4 => "ASTER",
                    5 => "WBNB",
                    _ => continue,
                };
                let current = config_store
                    .get("accepted_quotes")
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "BNB,CAKE,USDT,USD1,ASTER,WBNB".to_string());
                let mut parts: Vec<String> = current
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if let Some(pos) = parts.iter().position(|s| s == label) {
                    parts.remove(pos);
                } else {
                    parts.push(label.to_string());
                }
                // Keep deterministic order
                let mut ordered: Vec<String> = Vec::new();
                for k in ["BNB", "CAKE", "USDT", "USD1", "ASTER", "WBNB"] {
                    if parts.iter().any(|s| s == k) {
                        ordered.push(k.to_string());
                    }
                }
                config_store.insert("accepted_quotes".into(), ordered.join(","));
                let _ = save_autotrade_cache(config_store);
            }
        }
    }
}
