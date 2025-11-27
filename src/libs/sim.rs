use alloy::primitives::Address;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::Instant;

/// DEX type for a simulated trade
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DexType {
    V2,
    V3,
    FourMeme,
}

/// Status of a simulated position
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PositionStatus {
    Open,
    ClosedTP,
    ClosedSL,
    ClosedManual,
}

/// A simulated trading position
#[derive(Debug, Clone)]
pub struct SimPosition {
    pub pair_address: String,
    pub dex_type: DexType,
    pub base_token: String,
    pub quote_token: String,
    pub entry_price: f64,
    pub current_price: f64,
    pub buy_amount_wbnb: f64,
    pub remaining_amount_wbnb: f64,
    pub opened_at: Instant,
    pub closed_at: Option<Instant>,
    pub status: PositionStatus,
    pub liquidity_usd: Option<f64>,
    pub out_of_liq: bool,
    pub needs_liq_ack: bool,
    pub tp_pct: Option<f64>,
    pub sl_pct: Option<f64>,
    pub pnl_pct: f64,
    pub pnl_wbnb: f64,
    pub realized_pnl_wbnb: f64,
    pub frozen: bool,
}

impl SimPosition {
    pub fn new(
        pair_address: String,
        dex_type: DexType,
        base_token: String,
        quote_token: String,
        entry_price: f64,
        buy_amount_wbnb: f64,
        tp_pct: Option<f64>,
        sl_pct: Option<f64>,
    ) -> Self {
        Self {
            pair_address,
            dex_type,
            base_token,
            quote_token,
            entry_price,
            current_price: entry_price,
            buy_amount_wbnb,
            remaining_amount_wbnb: buy_amount_wbnb,
            opened_at: Instant::now(),
            closed_at: None,
            status: PositionStatus::Open,
            liquidity_usd: None,
            out_of_liq: false,
            needs_liq_ack: false,
            tp_pct,
            sl_pct,
            pnl_pct: 0.0,
            pnl_wbnb: 0.0,
            realized_pnl_wbnb: 0.0,
            frozen: false,
        }
    }

    pub fn update_liquidity(&mut self, liquidity: Option<f64>) {
        self.liquidity_usd = liquidity;
        if let Some(liq) = liquidity {
            if liq < 5.0 {
                // Fire a one-shot alert when crossing below the guard
                if !self.out_of_liq {
                    self.needs_liq_ack = true;
                }
                self.out_of_liq = true;
            } else {
                // Do not auto-clear if the alert has not been acknowledged yet
                if self.out_of_liq && self.needs_liq_ack {
                    return;
                }
                self.out_of_liq = false;
                self.needs_liq_ack = false;
            }
        }
    }

    /// Update position with new price and check TP/SL conditions
    pub fn update_price(&mut self, new_price: f64) -> bool {
        self.current_price = new_price;

        if self.entry_price <= 0.0 {
            return false;
        }

        self.pnl_pct = ((new_price / self.entry_price) - 1.0) * 100.0;
        // PnL on the still-open portion only
        self.pnl_wbnb = self.remaining_amount_wbnb * (self.pnl_pct / 100.0);

        // Frozen positions never auto-close on TP/SL
        if self.frozen {
            return false;
        }

        // Only auto-close on TP/SL if there is remaining amount
        if self.remaining_amount_wbnb > 0.0 {
            // Check TP condition
            if let Some(tp) = self.tp_pct {
                if self.pnl_pct >= tp {
                    self.close(PositionStatus::ClosedTP);
                    return true;
                }
            }

            // Check SL condition
            if let Some(sl) = self.sl_pct {
                if self.pnl_pct <= -sl {
                    self.close(PositionStatus::ClosedSL);
                    return true;
                }
            }
        }

        false
    }

    /// Execute a partial sell by fraction of remaining amount (0.0 < fraction <= 1.0).
    /// Returns realized PnL from this partial action.
    pub fn partial_sell_fraction(&mut self, fraction: f64) -> f64 {
        if !(fraction > 0.0 && fraction <= 1.0) || self.remaining_amount_wbnb <= 0.0 {
            return 0.0;
        }
        let sell_amount = self.remaining_amount_wbnb * fraction;
        let realized = sell_amount * (self.pnl_pct / 100.0);
        self.remaining_amount_wbnb -= sell_amount;
        if self.remaining_amount_wbnb.abs() < 1e-12 {
            self.remaining_amount_wbnb = 0.0;
        }
        self.realized_pnl_wbnb += realized;
        // Update open PnL on remaining
        self.pnl_wbnb = self.remaining_amount_wbnb * (self.pnl_pct / 100.0);
        realized
    }

    pub fn close(&mut self, status: PositionStatus) {
        self.status = status;
        self.closed_at = Some(Instant::now());
    }

    pub fn is_open(&self) -> bool {
        self.status == PositionStatus::Open
    }

    pub fn duration_secs(&self) -> u64 {
        let end = self.closed_at.unwrap_or_else(Instant::now);
        end.duration_since(self.opened_at).as_secs()
    }

    /// Total PnL including realized from partial sells plus current open PnL
    pub fn total_pnl_wbnb(&self) -> f64 {
        self.realized_pnl_wbnb + self.pnl_wbnb
    }
}

/// Simulation engine that tracks positions
pub struct SimEngine {
    positions: HashMap<String, SimPosition>,
    closed_positions: Vec<SimPosition>,
    max_positions: usize,
    // Track pairs we've "submitted buy tx" for (waiting for next price update)
    pending_buys: HashMap<String, (DexType, String, String, f64, Option<f64>, Option<f64>)>,
    // Tokens/pairs that must never be re-bought again (per session lifetime)
    do_not_rebuy: HashSet<String>,
    // Max hold duration in seconds (0 = disabled)
    max_hold_secs: u64,
    // Whether to enforce PnL threshold (50%) on Max Hold auto-close
    max_hold_pnl_enabled: bool,
}

impl SimEngine {
    pub fn new(max_positions: usize) -> Self {
        Self {
            positions: HashMap::new(),
            closed_positions: Vec::new(),
            max_positions,
            pending_buys: HashMap::new(),
            do_not_rebuy: HashSet::new(),
            max_hold_secs: 0,
            max_hold_pnl_enabled: true,
        }
    }

    /// Update max hold duration in seconds (0 disables)
    pub fn set_max_hold_secs(&mut self, secs: u64) {
        self.max_hold_secs = secs;
    }

    /// Enable/disable PnL gating on Max Hold (threshold fixed at 50%)
    pub fn set_max_hold_pnl_enabled(&mut self, enabled: bool) {
        self.max_hold_pnl_enabled = enabled;
    }

    /// Manually close a single open position by pair address. Returns the closed position.
    pub fn take_position(&mut self, pair_address: &str) -> Option<SimPosition> {
        if let Some(mut pos) = self.positions.remove(pair_address) {
            // On full take, realize remaining open PnL plus any previously realized partial PnL
            pos.pnl_wbnb = pos.realized_pnl_wbnb + pos.pnl_wbnb;
            pos.close(PositionStatus::ClosedManual);
            let closed = pos.clone();
            self.do_not_rebuy.insert(closed.pair_address.clone());
            self.closed_positions.push(pos);
            Some(closed)
        } else {
            None
        }
    }

    /// Partially sell an open position by a fraction (e.g., 0.1, 0.25, 0.5).
    /// Returns Some((realized_pnl, closed_now)) if position exists and not frozen.
    pub fn partial_take(&mut self, pair_address: &str, fraction: f64) -> Option<(f64, bool)> {
        if let Some(pos) = self.positions.get_mut(pair_address) {
            if pos.frozen {
                return None;
            }
            let realized = pos.partial_sell_fraction(fraction);
            let closed_now = pos.remaining_amount_wbnb == 0.0;
            // Do NOT remove or close the position on partial sells, even if remaining is 0.
            // Finalization happens only on explicit 'Take'.
            Some((realized, closed_now))
        } else {
            None
        }
    }

    /// Remove a position from management without closing stats; prevents immediate re-buy.
    pub fn remove_position(&mut self, pair_address: &str) -> bool {
        let mut removed = false;
        if self.positions.remove(pair_address).is_some() {
            removed = true;
        }
        if self.pending_buys.remove(pair_address).is_some() {
            removed = true;
        }
        if removed {
            self.do_not_rebuy.insert(pair_address.to_string());
        }
        removed
    }

    /// Manually close all open positions. Returns the list of closed positions.
    pub fn take_all(&mut self) -> Vec<SimPosition> {
        let keys: Vec<String> = self
            .positions
            .iter()
            .filter(|(_, p)| !p.frozen)
            .map(|(k, _)| k.clone())
            .collect();
        let mut closed_list: Vec<SimPosition> = Vec::with_capacity(keys.len());
        for k in keys.into_iter() {
            if let Some(mut pos) = self.positions.remove(&k) {
                // Realize any remaining open PnL plus prior partials
                pos.pnl_wbnb = pos.realized_pnl_wbnb + pos.pnl_wbnb;
                pos.close(PositionStatus::ClosedManual);
                self.do_not_rebuy.insert(pos.pair_address.clone());
                closed_list.push(pos.clone());
                self.closed_positions.push(pos);
            }
        }
        closed_list
    }

    /// Submit a "buy order" (will execute on next price update, simulating block delay)
    pub fn submit_buy(
        &mut self,
        pair_address: String,
        dex_type: DexType,
        base_token: String,
        quote_token: String,
        buy_amount: f64,
        tp_pct: Option<f64>,
        sl_pct: Option<f64>,
    ) -> bool {
        // Check if already have this pair or pending
        if self.positions.contains_key(&pair_address)
            || self.pending_buys.contains_key(&pair_address)
        {
            return false;
        }
        // Enforce global do-not-rebuy
        if self.do_not_rebuy.contains(&pair_address) {
            return false;
        }

        // Check max positions limit
        if self.positions.len() + self.pending_buys.len() >= self.max_positions {
            return false;
        }

        self.pending_buys.insert(
            pair_address,
            (
                dex_type,
                base_token,
                quote_token,
                buy_amount,
                tp_pct,
                sl_pct,
            ),
        );
        true
    }

    /// Update position or execute pending buy with new price
    /// `allow_close` controls whether TP/SL/MaxHold auto-closes are applied (true for sim, false when mirroring real trades for display).
    pub fn update_or_execute(
        &mut self,
        pair_address: &str,
        new_price: f64,
        liquidity: Option<f64>,
        allow_close: bool,
    ) -> Option<String> {
        // Check if this is a pending buy - execute it at this price (simulating block delay)
        if let Some((dex_type, base_token, quote_token, buy_amount, tp_pct, sl_pct)) =
            self.pending_buys.remove(pair_address)
        {
            let mut position = SimPosition::new(
                pair_address.to_string(),
                dex_type,
                base_token.clone(),
                quote_token,
                new_price, // Execute at NEXT price, not first price
                buy_amount,
                tp_pct,
                sl_pct,
            );
            position.update_liquidity(liquidity);
            self.positions.insert(pair_address.to_string(), position);
            return Some(format!(
                "EXECUTED buy for {} at {:.8} (simulated 1-block delay)",
                base_token, new_price
            ));
        }

        // Otherwise, update existing position
        if let Some(pos) = self.positions.get_mut(pair_address) {
            pos.update_liquidity(liquidity);
            let closed = pos.update_price(new_price);
            if closed && allow_close {
                // Move to closed positions
                if let Some(closed_pos) = self.positions.remove(pair_address) {
                    self.do_not_rebuy.insert(closed_pos.pair_address.clone());
                    self.closed_positions.push(closed_pos);
                }
            }
        }

        // Time-based auto-take: if max_hold reached and (PnL <= 50% when enabled), close (when not frozen)
        if self.max_hold_secs > 0 && allow_close {
            let should_close = if let Some(pos) = self.positions.get(pair_address) {
                if !pos.frozen
                    && pos.duration_secs() >= self.max_hold_secs
                    && pos.remaining_amount_wbnb > 0.0
                {
                    if self.max_hold_pnl_enabled {
                        pos.pnl_pct <= 50.0
                    } else {
                        true
                    }
                } else {
                    false
                }
            } else {
                false
            };
            if should_close {
                if let Some(mut pos) = self.positions.remove(pair_address) {
                    pos.close(PositionStatus::ClosedManual);
                    let msg = format!(
                        "⏰ MAX HOLD TAKE closed {} ({}) PnL: {:+.6} WBNB",
                        pos.base_token, pos.pair_address, pos.pnl_wbnb
                    );
                    self.do_not_rebuy.insert(pos.pair_address.clone());
                    self.closed_positions.push(pos);
                    return Some(msg);
                }
            }
        }

        None
    }

    /// Get all open positions sorted by entry time (oldest first)
    pub fn open_positions(&self) -> Vec<&SimPosition> {
        let mut positions: Vec<&SimPosition> = self.positions.values().collect();
        positions.sort_by_key(|p| p.opened_at);
        positions
    }

    /// Clone a single open position by pair address (if present).
    pub fn open_position(&self, pair_address: &str) -> Option<SimPosition> {
        self.positions.get(pair_address).cloned()
    }

    /// Check if we have a position or pending buy for this pair
    pub fn has_position_or_pending(&self, pair_address: &str) -> bool {
        self.positions.contains_key(pair_address)
            || self.pending_buys.contains_key(pair_address)
            || self.do_not_rebuy.contains(pair_address)
    }

    /// Get all closed positions
    pub fn closed_positions(&self) -> &[SimPosition] {
        &self.closed_positions
    }

    /// Get statistics
    pub fn stats(&self) -> SimStats {
        let total_trades = self.closed_positions.len();
        let winning_trades = self
            .closed_positions
            .iter()
            .filter(|p| p.pnl_wbnb > 0.0)
            .count();
        let losing_trades = self
            .closed_positions
            .iter()
            .filter(|p| p.pnl_wbnb < 0.0)
            .count();

        let total_pnl = self
            .closed_positions
            .iter()
            .map(|p| p.pnl_wbnb)
            .sum::<f64>();
        let total_pnl_open = self.positions.values().map(|p| p.pnl_wbnb).sum::<f64>();
        let realized_pnl_partial = self
            .positions
            .values()
            .map(|p| p.realized_pnl_wbnb)
            .sum::<f64>();
        let total_pnl_realized = total_pnl + realized_pnl_partial;

        let win_rate = if total_trades > 0 {
            (winning_trades as f64 / total_trades as f64) * 100.0
        } else {
            0.0
        };

        let avg_pnl = if total_trades > 0 {
            total_pnl / total_trades as f64
        } else {
            0.0
        };

        SimStats {
            total_trades,
            winning_trades,
            losing_trades,
            win_rate,
            total_pnl_closed: total_pnl,
            total_pnl_open,
            avg_pnl_per_trade: avg_pnl,
            open_positions: self.positions.len(),
            realized_pnl_partial,
            total_pnl_realized,
        }
    }

    /// Clear all positions (for reset)
    pub fn reset(&mut self) {
        self.positions.clear();
        self.closed_positions.clear();
        self.pending_buys.clear();
    }

    /// Update max positions limit (does not affect existing positions)
    pub fn update_max_positions(&mut self, new_max: usize) {
        self.max_positions = new_max;
    }

    /// Set freeze status for an open position. Returns true if updated.
    pub fn set_freeze(&mut self, pair_address: &str, frozen: bool) -> bool {
        if let Some(pos) = self.positions.get_mut(pair_address) {
            pos.frozen = frozen;
            return true;
        }
        false
    }

    /// Toggle freeze status. Returns Some(new_state) if position exists.
    pub fn toggle_freeze(&mut self, pair_address: &str) -> Option<bool> {
        if let Some(pos) = self.positions.get_mut(pair_address) {
            pos.frozen = !pos.frozen;
            return Some(pos.frozen);
        }
        None
    }

    /// Returns true if this position has an outstanding low-liquidity alert.
    pub fn position_needs_liq_ack(&self, pair_address: &str) -> bool {
        self.positions
            .get(pair_address)
            .map(|p| p.needs_liq_ack)
            .unwrap_or(false)
    }

    /// Acknowledge all pending low-liquidity alerts. Returns the count cleared.
    pub fn ack_all_liq_alerts(&mut self) -> usize {
        let mut cleared = 0usize;
        for pos in self.positions.values_mut() {
            if pos.needs_liq_ack {
                pos.needs_liq_ack = false;
                cleared += 1;
            }
        }
        cleared
    }

    pub fn has_pending_liq_alert(&self) -> bool {
        self.positions.values().any(|p| p.needs_liq_ack)
    }

    /// Mirror a real buy directly into the sim engine (without pending delay).
    pub fn add_real_position(
        &mut self,
        pair_address: String,
        dex_type: DexType,
        base_token: String,
        quote_token: String,
        entry_price: f64,
        buy_amount_wbnb: f64,
        tp_pct: Option<f64>,
        sl_pct: Option<f64>,
        liquidity_usd: Option<f64>,
    ) {
        if self.positions.contains_key(&pair_address) {
            return;
        }
        let mut pos = SimPosition::new(
            pair_address.clone(),
            dex_type,
            base_token,
            quote_token,
            entry_price,
            buy_amount_wbnb,
            tp_pct,
            sl_pct,
        );
        pos.update_liquidity(liquidity_usd);
        self.positions.insert(pair_address, pos);
    }
}

#[derive(Debug, Clone)]
pub struct SimStats {
    pub total_trades: usize,
    pub winning_trades: usize,
    pub losing_trades: usize,
    pub win_rate: f64,
    pub total_pnl_closed: f64,
    pub total_pnl_open: f64,
    pub avg_pnl_per_trade: f64,
    pub open_positions: usize,
    /// Sum of realized PnL from partial sells on still-open positions
    pub realized_pnl_partial: f64,
    /// Closed PnL + realized partials (excludes current unrealized open PnL)
    pub total_pnl_realized: f64,
}
