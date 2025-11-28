use {
    crate::app::pair_state::detect_source,
    crate::app::pair_state::extract_price_f64,
    crate::app::pair_state::PairSource,
    crate::app::pair_streams::pair_metrics,
    crate::libs::lookup::save_log_to_file,
    crate::libs::sim::{DexType, SimEngine},
    crate::libs::tui::ConfigStore,
    crate::libs::ws::pairs::PairInfo,
    crate::shared::should_avoid_name,
    anyhow::{anyhow, Result},
    fourmeme::abi::ITokenManagerHelper3,
    fourmeme::addresses::TOKEN_MANAGER_HELPER_3,
    pancakes::pancake::pancake_swap::addresses::PANCAKE_V3_SWAP_ROUTER,
    pancakes::pancake::pancake_swap_v2::addresses::PANCAKE_V2_ROUTER,
    std::collections::{HashMap, HashSet},
    std::future::Future,
    std::pin::Pin,
    std::sync::Arc,
    std::time::Instant,
    tokio::sync::{mpsc, RwLock},
    tokio::time::Duration,
};

use crate::router::FmRouter;
use crate::routy::{v2 as routy_v2, v3 as routy_v3, wbnb};
use alloy::primitives::utils::parse_units;
use alloy::primitives::{Address, U256};
use alloy::providers::{Provider, WalletProvider};
use dashmap::DashMap;
use once_cell::sync::Lazy;
use pancakes::pancake::pancake_swap::addresses::WBNB;
use pancakes::pancake::pancake_swap::PancakeV3;
use pancakes::pancake::pancake_swap_v2::PancakeV2;
use tokio::sync::Mutex;

alloy::sol! {
    #[sol(rpc)]
    interface IERC20Lite {
        function balanceOf(address owner) view returns (uint256);
    }

    #[sol(rpc)]
    interface IERC20Approve {
        function allowance(address owner, address spender) view returns (uint256);
        function approve(address spender, uint256 value) returns (bool);
    }
}

async fn safe_balance_of<P>(provider: P, token: Address, owner: Address) -> U256
where
    P: Provider + Clone,
{
    let c = IERC20Lite::new(token, provider);
    c.balanceOf(owner).call().await.unwrap_or(U256::ZERO)
}

pub fn pair_key_addr(a: alloy::primitives::Address) -> String {
    format!("{:#x}", a).to_ascii_lowercase()
}

pub fn pair_key_str(s: &str) -> String {
    s.trim().to_ascii_lowercase()
}

const DEFAULT_GAS_WEI: u128 = 1_000_000_000; // 1 gwei
const GWEI_DECIMALS: u8 = 9;

fn gas_price_wei_u128(config_store: &ConfigStore) -> u128 {
    config_store
        .get("max_gwei")
        .and_then(|v| parse_units(v.as_str(), GWEI_DECIMALS).ok())
        .and_then(|wei| wei.try_into().ok())
        .filter(|wei| *wei > 0)
        .unwrap_or(DEFAULT_GAS_WEI)
}

fn wei_to_bnb(wei: U256) -> f64 {
    wei.try_into()
        .map(|v: u128| v as f64 / 1e18f64)
        .unwrap_or(0.0)
}

fn wrap_ratio_pct_value(config_store: &ConfigStore) -> u64 {
    config_store
        .get("wrap_ratio_pct")
        .and_then(|v| v.parse::<u64>().ok())
        .map(|v| v.clamp(1, 99))
        .unwrap_or(80)
}

fn record_buy_failure(trader: &mut RealTrader, pair_key: &str) -> u32 {
    let cnt = BUY_FAILS
        .entry(pair_key.to_string())
        .and_modify(|v| *v += 1)
        .or_insert(1);
    if *cnt >= 3 {
        trader.block_rebuy(pair_key);
        save_log_to_file(&format!(
            "[trade] SKIP {}: marked do-not-rebuy after {} failed attempts",
            pair_key, *cnt
        ));
    }
    *cnt
}

fn clear_buy_failures(pair_key: &str) {
    BUY_FAILS.remove(pair_key);
}

fn contains_cjk(s: &str) -> bool {
    s.chars().any(|c| {
        let u = c as u32;
        (0x4E00..=0x9FFF).contains(&u)
            || (0x3400..=0x4DBF).contains(&u)
            || (0x20000..=0x2A6DF).contains(&u)
            || (0x2A700..=0x2B73F).contains(&u)
            || (0x2B740..=0x2B81F).contains(&u)
            || (0x2B820..=0x2CEAF).contains(&u)
    })
}

async fn wait_for_balance_drop<P>(
    provider: P,
    token: Address,
    owner: Address,
    bal_before: U256,
    retries: usize,
    delay_ms: u64,
) -> Option<U256>
where
    P: Provider + Clone,
{
    for _ in 0..retries {
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        let bal_after = safe_balance_of(provider.clone(), token, owner).await;
        if bal_after < bal_before {
            return Some(bal_after);
        }
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum AllowanceMarket {
    V2,
    V3,
    FourMeme,
}

impl AllowanceMarket {
    fn as_str(&self) -> &'static str {
        match self {
            AllowanceMarket::V2 => "v2",
            AllowanceMarket::V3 => "v3",
            AllowanceMarket::FourMeme => "fm",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct AllowanceKey {
    token: Address,
    market: AllowanceMarket,
}

type BoxAllowFuture = Pin<Box<dyn Future<Output = bool> + Send + 'static>>;

struct AllowanceJob {
    key: AllowanceKey,
    fut: BoxAllowFuture,
}

struct AllowanceWorker {
    tx: mpsc::Sender<AllowanceJob>,
}

impl AllowanceWorker {
    fn new() -> Self {
        let (tx, mut rx) = mpsc::channel::<AllowanceJob>(64);
        tokio::spawn(async move {
            let mut approved: HashSet<AllowanceKey> = HashSet::new();
            while let Some(job) = rx.recv().await {
                if approved.contains(&job.key) {
                    continue;
                }
                let ok = job.fut.await;
                if ok {
                    approved.insert(job.key);
                }
            }
        });
        Self { tx }
    }

    async fn enqueue(&self, job: AllowanceJob) {
        let _ = self.tx.send(job).await;
    }
}

static ALLOWANCE_WORKER: Lazy<AllowanceWorker> = Lazy::new(AllowanceWorker::new);
static BUY_FAILS: Lazy<DashMap<String, u32>> = Lazy::new(DashMap::new);

fn dex_to_market(dex: DexType) -> Option<AllowanceMarket> {
    match dex {
        DexType::V2 => Some(AllowanceMarket::V2),
        DexType::V3 => Some(AllowanceMarket::V3),
        DexType::FourMeme => Some(AllowanceMarket::FourMeme),
    }
}

fn queue_allowance_job<P>(provider: P, dex: DexType, token: Address, gas_price_wei: u128)
where
    P: Provider + Clone + WalletProvider + Send + Sync + 'static,
{
    let Some(market) = dex_to_market(dex) else {
        return;
    };
    let job = make_allowance_job(provider, market, token, gas_price_wei);
    tokio::spawn(async move {
        ALLOWANCE_WORKER.enqueue(job).await;
    });
}

fn make_allowance_job<P>(
    provider: P,
    market: AllowanceMarket,
    token: Address,
    gas_price_wei: u128,
) -> AllowanceJob
where
    P: Provider + Clone + WalletProvider + Send + Sync + 'static,
{
    let key = AllowanceKey { token, market };
    let fut = Box::pin(async move {
        match send_allowance_with_retry(provider, market, token, gas_price_wei, 3).await {
            Ok(_) => {
                save_log_to_file(&format!(
                    "[allow] ✓ approved {:#x} for {}",
                    token,
                    market.as_str()
                ));
                true
            }
            Err(e) => {
                save_log_to_file(&format!(
                    "[allow] ✗ approval failed {:#x} for {}: {}",
                    token,
                    market.as_str(),
                    e
                ));
                false
            }
        }
    });
    AllowanceJob { key, fut }
}

async fn approve_once<P>(
    provider: P,
    token: Address,
    spender: Address,
    min_needed: U256,
    gas_price_wei: u128,
) -> Result<()>
where
    P: Provider + Clone + WalletProvider + Send + Sync + 'static,
{
    let from = provider.default_signer_address();
    let erc20 = IERC20Approve::new(token, provider.clone());
    let allowance = erc20
        .allowance(from, spender)
        .call()
        .await
        .unwrap_or(U256::ZERO);
    let floor = U256::from(1_000_000u64);
    if allowance >= min_needed.max(floor) {
        return Ok(());
    }

    let mut call = erc20.approve(spender, U256::MAX).from(from);
    if gas_price_wei > 0 {
        call = call.gas_price(gas_price_wei);
    }
    let pending = call.send().await?;
    let tx = *pending.tx_hash();
    let _ = pending.get_receipt().await;
    save_log_to_file(&format!(
        "[allow] sent approval for {:#x} -> {:#x} tx={}",
        token, spender, tx
    ));
    Ok(())
}

async fn send_allowance<P>(
    provider: P,
    market: AllowanceMarket,
    token: Address,
    gas_price_wei: u128,
) -> Result<()>
where
    P: Provider + Clone + WalletProvider + Send + Sync + 'static,
{
    match market {
        AllowanceMarket::V2 => {
            approve_once(provider, token, PANCAKE_V2_ROUTER, U256::MAX, gas_price_wei).await
        }
        AllowanceMarket::V3 => {
            approve_once(
                provider,
                token,
                PANCAKE_V3_SWAP_ROUTER,
                U256::MAX,
                gas_price_wei,
            )
            .await
        }
        AllowanceMarket::FourMeme => {
            let helper = ITokenManagerHelper3::new(TOKEN_MANAGER_HELPER_3, provider.clone());
            let info = helper.getTokenInfo(token).call().await?;
            if info.tokenManager.is_zero() {
                return Err(anyhow!("tokenManager is zero for token {:#x}", token));
            }
            for spender in [info.tokenManager, TOKEN_MANAGER_HELPER_3] {
                approve_once(provider.clone(), token, spender, U256::MAX, gas_price_wei).await?;
            }
            Ok(())
        }
    }
}

async fn send_allowance_with_retry<P>(
    provider: P,
    market: AllowanceMarket,
    token: Address,
    gas_price_wei: u128,
    attempts: usize,
) -> Result<()>
where
    P: Provider + Clone + WalletProvider + Send + Sync + 'static,
{
    let mut last_err: Option<anyhow::Error> = None;
    for i in 0..attempts {
        match send_allowance(provider.clone(), market, token, gas_price_wei).await {
            Ok(_) => return Ok(()),
            Err(e) => {
                last_err = Some(e);
                // Small backoff before retry
                tokio::time::sleep(Duration::from_millis(300 * (i as u64 + 1))).await;
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow!("allowance failed")))
}

pub(crate) async fn ensure_sell_allowance<P>(
    provider: P,
    dex: DexType,
    token: Address,
    amount_needed: U256,
    gas_price_wei: u128,
) -> Result<()>
where
    P: Provider + Clone + WalletProvider + Send + Sync + 'static,
{
    match dex {
        DexType::V2 => {
            approve_once(
                provider,
                token,
                PANCAKE_V2_ROUTER,
                amount_needed,
                gas_price_wei,
            )
            .await
        }
        DexType::V3 => {
            approve_once(
                provider,
                token,
                PANCAKE_V3_SWAP_ROUTER,
                amount_needed,
                gas_price_wei,
            )
            .await
        }
        DexType::FourMeme => {
            send_allowance_with_retry(provider, AllowanceMarket::FourMeme, token, gas_price_wei, 2)
                .await
        }
    }
}

async fn ensure_wbnb_topup<P>(
    provider: P,
    from: Address,
    amount_needed: U256,
    wrap_ratio_pct: u64,
    pair_label: &str,
) -> bool
where
    P: Provider + Clone + WalletProvider + Send + Sync + 'static,
{
    let wbnb_balance = safe_balance_of(provider.clone(), WBNB, from).await;
    if wbnb_balance >= amount_needed {
        return true;
    }

    let missing = amount_needed.saturating_sub(wbnb_balance);
    let bnb_balance = provider.get_balance(from).await.unwrap_or(U256::ZERO);
    let max_wrap = bnb_balance
        .checked_mul(U256::from(wrap_ratio_pct))
        .and_then(|v| v.checked_div(U256::from(100u64)))
        .unwrap_or(U256::ZERO);
    let wrap_amount = missing.min(max_wrap);

    if wrap_amount.is_zero() {
        save_log_to_file(&format!(
            "[trade] SKIP {}: need {:.6} WBNB, have {:.6}, BNB {:.6} (wrap ratio {}%)",
            pair_label,
            wei_to_bnb(amount_needed),
            wei_to_bnb(wbnb_balance),
            wei_to_bnb(bnb_balance),
            wrap_ratio_pct,
        ));
        return false;
    }

    match wbnb::wrap_bnb(provider.clone(), from, wrap_amount).await {
        Ok(tx) => {
            save_log_to_file(&format!(
                "[trade] wrap {:.6} BNB -> WBNB for {} (ratio {}%) tx={:#x}",
                wei_to_bnb(wrap_amount),
                pair_label,
                wrap_ratio_pct,
                tx,
            ));
            let new_balance = safe_balance_of(provider.clone(), WBNB, from).await;
            if new_balance < amount_needed {
                save_log_to_file(&format!(
                    "[trade] SKIP {}: after wrap WBNB {:.6} < needed {:.6}",
                    pair_label,
                    wei_to_bnb(new_balance),
                    wei_to_bnb(amount_needed),
                ));
                return false;
            }
        }
        Err(e) => {
            save_log_to_file(&format!(
                "[trade] SKIP {}: failed to wrap BNB: {}",
                pair_label, e
            ));
            return false;
        }
    }

    true
}

#[derive(Clone, Debug)]
struct RealPosition {
    pair_address: String,
    dex_type: DexType,
    token_out: Address,
    base_symbol: String,
    entry_price: f64,
    buy_amount_bnb: f64,
    opened_at: Instant,
}

#[derive(Debug, Clone, Copy)]
enum SellTrigger {
    TakeProfit(f64),
    StopLoss(f64),
    MaxHold(u64),
    Manual,
}

#[derive(Clone, Debug)]
struct SellPlan {
    pair_key: String,
    dex_type: DexType,
    token_out: Address,
    base_symbol: String,
    spent_bnb: f64,
    pnl_pct: f64,
    trigger: SellTrigger,
    percent_points: u32,
}

#[derive(Default)]
struct RealTrader {
    positions: HashMap<String, RealPosition>,
    closing: HashSet<String>,
    do_not_rebuy: HashSet<String>,
}

static REAL_TRADER: Lazy<Mutex<RealTrader>> = Lazy::new(|| Mutex::new(RealTrader::default()));

impl RealTrader {
    fn has_position_or_blocked(&self, pair_key: &str) -> bool {
        self.positions.contains_key(pair_key)
            || self.closing.contains(pair_key)
            || self.do_not_rebuy.contains(pair_key)
    }

    fn record_buy(&mut self, pos: RealPosition) {
        self.closing.remove(&pos.pair_address);
        self.do_not_rebuy.remove(&pos.pair_address);
        self.positions.insert(pos.pair_address.clone(), pos);
    }

    fn reserve_close(&mut self, pair_key: &str) -> bool {
        if self.closing.contains(pair_key) {
            return false;
        }
        if !self.positions.contains_key(pair_key) {
            return false;
        }
        self.closing.insert(pair_key.to_string());
        true
    }

    fn abort_close(&mut self, pair_key: &str) {
        self.closing.remove(pair_key);
    }

    fn finish_sell(&mut self, pair_key: &str) {
        self.closing.remove(pair_key);
        if let Some(pos) = self.positions.remove(pair_key) {
            self.do_not_rebuy.insert(pos.pair_address);
        }
    }

    fn remove_position(&mut self, pair_key: &str) -> bool {
        self.closing.remove(pair_key);
        if let Some(pos) = self.positions.remove(pair_key) {
            self.do_not_rebuy.insert(pos.pair_address);
            true
        } else {
            false
        }
    }

    fn clear_all_closing(&mut self) {
        self.closing.clear();
    }

    fn block_rebuy(&mut self, pair_key: &str) {
        self.do_not_rebuy.insert(pair_key.to_string());
    }

    fn sell_decision(
        &mut self,
        pair_key: &str,
        current_price: f64,
        config_store: &ConfigStore,
    ) -> Option<SellPlan> {
        if self.closing.contains(pair_key) {
            return None;
        }

        let pos = self.positions.get(pair_key)?;
        if pos.entry_price <= 0.0 {
            return None;
        }

        let pnl_pct = ((current_price / pos.entry_price) - 1.0) * 100.0;

        let tp_enabled = config_store
            .get("tp_enabled")
            .map(|v| v.as_str() == "true")
            .unwrap_or(false);
        let tp_pct = if tp_enabled {
            config_store
                .get("tp_pct")
                .and_then(|v| v.parse::<f64>().ok())
        } else {
            None
        };

        let sl_enabled = config_store
            .get("sl_enabled")
            .map(|v| v.as_str() == "true")
            .unwrap_or(false);
        let sl_pct = if sl_enabled {
            config_store
                .get("sl_pct")
                .and_then(|v| v.parse::<f64>().ok())
        } else {
            None
        };

        let mut trigger: Option<SellTrigger> = None;
        if let Some(tp) = tp_pct {
            if pnl_pct >= tp {
                trigger = Some(SellTrigger::TakeProfit(tp));
            }
        }
        if trigger.is_none() {
            if let Some(sl) = sl_pct {
                if pnl_pct <= -sl {
                    trigger = Some(SellTrigger::StopLoss(sl));
                }
            }
        }

        if trigger.is_none() {
            let max_hold_secs = config_store
                .get("max_hold_secs")
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0);
            if max_hold_secs > 0 && pos.opened_at.elapsed().as_secs() >= max_hold_secs {
                let max_hold_pnl_enabled = config_store
                    .get("max_hold_pnl")
                    .map(|v| v.as_str() == "true")
                    .unwrap_or(true);
                if !max_hold_pnl_enabled || pnl_pct <= 50.0 {
                    trigger = Some(SellTrigger::MaxHold(max_hold_secs));
                }
            }
        }

        let tr = trigger?;
        // reserve close BEFORE returning plan (prevents double-sells)
        self.closing.insert(pair_key.to_string());

        Some(SellPlan {
            pair_key: pair_key.to_string(),
            dex_type: pos.dex_type,
            token_out: pos.token_out,
            base_symbol: pos.base_symbol.clone(),
            spent_bnb: pos.buy_amount_bnb,
            pnl_pct,
            trigger: tr,
            percent_points: 100,
        })
    }
}

fn describe_trigger(trigger: &SellTrigger) -> String {
    match trigger {
        SellTrigger::TakeProfit(tp) => format!("TP {:.2}%", tp),
        SellTrigger::StopLoss(sl) => format!("SL -{:.2}%", sl),
        SellTrigger::MaxHold(secs) => format!("Max hold {}s", secs),
        SellTrigger::Manual => "Manual".to_string(),
    }
}

async fn execute_sell_plan<P>(
    plan: &SellPlan,
    provider: P,
    config_store: &ConfigStore,
) -> Result<()>
where
    P: Provider + Clone + WalletProvider + Send + Sync + 'static,
{
    let from = provider.default_signer_address();
    let bal_before = safe_balance_of(provider.clone(), plan.token_out, from).await;
    if bal_before.is_zero() {
        save_log_to_file(&format!(
            "[trade] skip sell {} ({}) reason={} token_balance=0",
            plan.base_symbol,
            plan.pair_key,
            describe_trigger(&plan.trigger)
        ));
        return Ok(());
    }
    let percent_bps: u16 = (plan.percent_points.min(100) as u16).saturating_mul(100);
    let gas_price_wei = gas_price_wei_u128(config_store);
    let gas_price_wei_override = U256::from(gas_price_wei);
    let amount_in = bal_before * U256::from(percent_bps) / U256::from(10_000u64);
    if amount_in.is_zero() {
        save_log_to_file(&format!(
            "[trade] skip sell {} ({}) reason={} computed_amount=0",
            plan.base_symbol,
            plan.pair_key,
            describe_trigger(&plan.trigger)
        ));
        return Ok(());
    }
    match plan.dex_type {
        DexType::V2 => {
            let pancake = PancakeV2::new(provider.clone());
            let token_in_s = format!("{:#x}", plan.token_out);
            ensure_sell_allowance(
                provider.clone(),
                DexType::V2,
                plan.token_out,
                amount_in,
                gas_price_wei,
            )
            .await?;
            let (_quoted, tx) = routy_v2::sell_pct_to_wbnb(
                &pancake,
                from,
                token_in_s.as_str(),
                percent_bps,
                Some(gas_price_wei),
            )
            .await?;
            let bal_after = safe_balance_of(provider.clone(), plan.token_out, from).await;
            let final_after = if bal_after < bal_before {
                Some(bal_after)
            } else {
                wait_for_balance_drop(provider.clone(), plan.token_out, from, bal_before, 4, 400)
                    .await
            };
            let _bal_after = match final_after {
                Some(b) => b,
                None => {
                    save_log_to_file(&format!(
                        "[trade] ✗ V2 SELL {} ({}) tx={} but token balance did not decrease (before={} after={})",
                        plan.base_symbol,
                        plan.pair_key,
                        tx,
                        bal_before,
                        bal_after
                    ));
                    return Err(anyhow!("sell tx mined but token balance did not decrease"));
                }
            };
            save_log_to_file(&format!(
                "[trade] ✓ V2 SELL {} ({}) @ {:+.2}% reason={} size:{:.6} BNB pct:{}",
                plan.base_symbol,
                plan.pair_key,
                plan.pnl_pct,
                describe_trigger(&plan.trigger),
                plan.spent_bnb,
                plan.percent_points,
            ));
            save_log_to_file(&format!("[trade] tx={}", tx));
        }
        DexType::V3 => {
            let pancake = PancakeV3::new(provider.clone());
            let token_in_s = format!("{:#x}", plan.token_out);
            ensure_sell_allowance(
                provider.clone(),
                DexType::V3,
                plan.token_out,
                amount_in,
                gas_price_wei,
            )
            .await?;
            let (_quoted, tx) = routy_v3::sell_pct_to_wbnb(
                &pancake,
                from,
                token_in_s.as_str(),
                percent_bps,
                Some(gas_price_wei),
            )
            .await?;
            let bal_after = safe_balance_of(provider.clone(), plan.token_out, from).await;
            let final_after = if bal_after < bal_before {
                Some(bal_after)
            } else {
                wait_for_balance_drop(provider.clone(), plan.token_out, from, bal_before, 4, 400)
                    .await
            };
            let _bal_after = match final_after {
                Some(b) => b,
                None => {
                    save_log_to_file(&format!(
                        "[trade] ✗ V3 SELL {} ({}) tx={} but token balance did not decrease (before={} after={})",
                        plan.base_symbol,
                        plan.pair_key,
                        tx,
                        bal_before,
                        bal_after
                    ));
                    return Err(anyhow!("sell tx mined but token balance did not decrease"));
                }
            };
            save_log_to_file(&format!(
                "[trade] ✓ V3 SELL {} ({}) @ {:+.2}% reason={} size:{:.6} BNB pct:{}",
                plan.base_symbol,
                plan.pair_key,
                plan.pnl_pct,
                describe_trigger(&plan.trigger),
                plan.spent_bnb,
                plan.percent_points,
            ));
            save_log_to_file(&format!("[trade] tx={}", tx));
        }
        DexType::FourMeme => {
            let router = FmRouter::new(provider.clone());
            ensure_sell_allowance(
                provider.clone(),
                DexType::FourMeme,
                plan.token_out,
                amount_in,
                gas_price_wei,
            )
            .await?;
            let sell_call = router.sell_percent_pct(
                from,
                plan.token_out,
                plan.percent_points.max(1),
                Some(gas_price_wei_override),
            );
            let sell_res = tokio::time::timeout(Duration::from_secs(20), sell_call)
                .await
                .map_err(|_| {
                    anyhow!(
                        "sell timeout for token {:#x} pct {}",
                        plan.token_out,
                        plan.percent_points
                    )
                })??;
            let (_est, tx) = sell_res;
            let bal_after = safe_balance_of(provider.clone(), plan.token_out, from).await;
            let final_after = if bal_after < bal_before {
                Some(bal_after)
            } else {
                wait_for_balance_drop(provider.clone(), plan.token_out, from, bal_before, 6, 400)
                    .await
            };
            let _bal_after = match final_after {
                Some(b) => b,
                None => {
                    save_log_to_file(&format!(
                        "[trade] ✗ FM SELL {} ({}) tx={:?} but token balance did not decrease (before={} after={})",
                        plan.base_symbol,
                        plan.pair_key,
                        tx,
                        bal_before,
                        bal_after
                    ));
                    return Err(anyhow!("sell tx mined but token balance did not decrease"));
                }
            };
            save_log_to_file(&format!(
                "[trade] ✓ FM SELL {} ({}) @ {:+.2}% reason={} size:{:.6} BNB pct:{}",
                plan.base_symbol,
                plan.pair_key,
                plan.pnl_pct,
                describe_trigger(&plan.trigger),
                plan.spent_bnb,
                plan.percent_points,
            ));
            save_log_to_file(&format!("[trade] tx={:?}", tx));
        }
    }
    Ok(())
}

pub async fn manual_sell<P>(
    pair_address: &str,
    percent_points: u32,
    provider: P,
    sim_engine: &Arc<tokio::sync::Mutex<SimEngine>>,
    sold_pairs: Option<&Arc<RwLock<HashSet<String>>>>,
    config_store: &ConfigStore,
) -> Result<bool>
where
    P: Provider + Clone + WalletProvider + Send + Sync + 'static,
{
    let pair_address = pair_key_str(pair_address);
    let pct = percent_points.clamp(1, 100);

    let plan = {
        let mut trader = REAL_TRADER.lock().await;

        if !trader.positions.contains_key(&pair_address) {
            save_log_to_file(&format!(
                "[trade] manual sell ignored: no real position {}",
                pair_address
            ));
            return Ok(false);
        }

        if pct >= 100 && !trader.reserve_close(&pair_address) {
            save_log_to_file(&format!(
                "[trade] manual sell ignored: already closing {}",
                pair_address
            ));
            return Ok(false);
        }

        let pos = trader.positions.get(&pair_address).cloned().unwrap();
        let pnl_pct = {
            let se = sim_engine.lock().await;
            se.open_position(&pair_address)
                .map(|p| p.pnl_pct)
                .unwrap_or(0.0)
        };

        SellPlan {
            pair_key: pair_address.to_string(),
            dex_type: pos.dex_type,
            token_out: pos.token_out,
            base_symbol: pos.base_symbol.clone(),
            spent_bnb: pos.buy_amount_bnb,
            pnl_pct,
            trigger: SellTrigger::Manual,
            percent_points: pct,
        }
    };

    let exec_res = execute_sell_plan(&plan, provider.clone(), config_store).await;

    if pct >= 100 {
        let mut trader = REAL_TRADER.lock().await;
        match &exec_res {
            Ok(_) => trader.finish_sell(&pair_address),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("TRANSFER_FROM_FAILED")
                    || msg.contains("balance is zero, nothing to sell")
                {
                    trader.abort_close(&pair_address);
                } else {
                    trader.abort_close(&pair_address);
                }
            }
        }
    }

    exec_res?;

    {
        let mut se = sim_engine.lock().await;
        if pct >= 100 {
            if let Some(pos) = se.take_position(&pair_address) {
                save_log_to_file(&format!(
                    "[trade] mirror close {} ({}) PnL: {:+.6} WBNB",
                    pos.base_token, pos.pair_address, pos.pnl_wbnb
                ));
                if let Some(sp) = sold_pairs {
                    let mut s = sp.write().await;
                    s.insert(pos.pair_address.clone());
                }
            }
        } else {
            let fraction = (pct as f64) / 100.0;
            if let Some((realized, closed)) = se.partial_take(&pair_address, fraction) {
                save_log_to_file(&format!(
                    "[trade] mirror partial {}% ({}) realized: {:+.6} WBNB{}",
                    pct,
                    pair_address,
                    realized,
                    if closed {
                        " (remaining 0%, Take to close)"
                    } else {
                        ""
                    }
                ));
            }
        }
    }

    Ok(true)
}

pub async fn manual_sell_all<P>(
    provider: P,
    sim_engine: &Arc<tokio::sync::Mutex<SimEngine>>,
    sold_pairs: Option<&Arc<RwLock<HashSet<String>>>>,
    config_store: &ConfigStore,
) -> Result<()>
where
    P: Provider + Clone + WalletProvider + Send + Sync + 'static,
{
    let keys: Vec<String> = {
        let mut trader = REAL_TRADER.lock().await;
        trader.clear_all_closing();
        trader.positions.keys().cloned().collect()
    };
    for k in keys {
        match manual_sell(
            &k,
            100,
            provider.clone(),
            sim_engine,
            sold_pairs,
            config_store,
        )
        .await
        {
            Ok(true) => {
                // successfully sent a sell tx
            }
            Ok(false) => {
                // no matching real position (very rare, but log for clarity)
                save_log_to_file(&format!(
                    "[trade] TAKE ALL: skipped {} (no real position)",
                    k
                ));
            }
            Err(e) => {
                save_log_to_file(&format!("[trade] TAKE ALL: sell failed for {}: {}", k, e));
            }
        }
    }
    Ok(())
}

/// Remove a position from management (hide from UI and stop auto-sell/buy for it).
pub async fn manual_remove_position(
    pair_address: &str,
    sim_engine: &Arc<tokio::sync::Mutex<SimEngine>>,
) -> Result<bool> {
    let pair_address = pair_key_str(pair_address);
    let mut removed = false;
    {
        let mut trader = REAL_TRADER.lock().await;
        removed |= trader.remove_position(&pair_address);
    }
    {
        let mut se = sim_engine.lock().await;
        removed |= se.remove_position(&pair_address);
    }
    if removed {
        save_log_to_file(&format!("[trade] manual REMOVE applied: {}", pair_address));
    } else {
        save_log_to_file(&format!(
            "[trade] manual REMOVE ignored: {} (no position)",
            pair_address
        ));
    }
    Ok(removed)
}

pub async fn auto_trade(
    l1: String,
    pair_info: PairInfo,
    buy_count: u32,
    sim_mode: bool,
    sim_engine: &mut SimEngine,
    config_store: &ConfigStore,
) -> Result<()> {
    if l1.contains("Price: ?") {
        return Ok(());
    }
    let price_opt = extract_price_f64(&l1);
    let src = detect_source(&l1);

    // Trade decision only (state updates moved to pair_streams::update_pairs_state)
    if sim_mode {
        if let Some(current_price) = price_opt {
            let pair_addr_str = format!("{:#x}", pair_info.pair);

            // Only consider buying if we don't already have position/pending
            if !sim_engine.has_position_or_pending(&pair_addr_str) {
                let enabled = config_store
                    .get("enabled")
                    .map(|v| v.as_str() == "true")
                    .unwrap_or(false);

                if enabled && current_price > 0.0 {
                    let avoid_cn = config_store
                        .get("avoid_chinese")
                        .map(|v| v.as_str() == "true")
                        .unwrap_or(false);
                    if avoid_cn
                        && (contains_cjk(&pair_info.symbol_base)
                            || contains_cjk(&pair_info.symbol_quote))
                    {
                        return Ok(());
                    }
                    if should_avoid_name(&pair_info.symbol_base)
                        || should_avoid_name(&pair_info.symbol_quote)
                    {
                        return Ok(());
                    }
                    let freshness_secs = config_store
                        .get("freshness_secs")
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(30);
                    let min_pnl_pct = config_store
                        .get("min_pnl_pct")
                        .and_then(|v| v.parse::<f64>().ok())
                        .unwrap_or(100.0);

                    // 1. Check if quote token is accepted
                    let accepted_quotes_str = config_store
                        .get("accepted_quotes")
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "BNB,CAKE,USDT,USD1,ASTER,WBNB".to_string());
                    let accepted_quotes: Vec<String> = accepted_quotes_str
                        .split(',')
                        .map(|s| s.trim().to_uppercase())
                        .collect();

                    // quote check
                    if !accepted_quotes.contains(&pair_info.symbol_quote.to_uppercase()) {
                        return Ok(());
                    }

                    // liquidity check
                    if src != PairSource::FourMeme {
                        let min_liquidity = config_store
                            .get("min_liquidity")
                            .and_then(|v| v.parse::<f64>().ok())
                            .unwrap_or(1000.0);
                        let liq_threshold = min_liquidity.max(5.0);

                        if let Some(liq) = pair_info.liquidity_usd {
                            if liq < liq_threshold {
                                save_log_to_file(&format!(
                                    "[sim] REJECTED {}: Liq ${:.0} < min ${:.0}",
                                    pair_info.symbol_base, liq, liq_threshold
                                ));
                                return Ok(());
                            }
                        } else {
                            return Ok(()); // No liquidity data
                        }
                    }

                    // minimum buys check
                    let min_buys = config_store
                        .get("min_buys")
                        .and_then(|v| v.parse::<u32>().ok())
                        .unwrap_or(3);

                    if buy_count < min_buys {
                        save_log_to_file(&format!(
                            "[sim] WAITING {}: {} buys < min {}",
                            pair_info.symbol_base, buy_count, min_buys
                        ));
                        return Ok(());
                    }

                    let dex_enabled = match src {
                        PairSource::V2 => config_store
                            .get("dexes")
                            .map(|v| v.as_str().contains("v2"))
                            .unwrap_or(false),
                        PairSource::V3 => config_store
                            .get("dexes")
                            .map(|v| v.as_str().contains("v3"))
                            .unwrap_or(false),
                        PairSource::FourMeme => config_store
                            .get("dexes")
                            .map(|v| v.as_str().contains("fm"))
                            .unwrap_or(false),
                        _ => false,
                    };

                    if dex_enabled {
                        let buy_amount = config_store
                            .get("buy_amount_wbnb")
                            .and_then(|v| v.parse::<f64>().ok())
                            .unwrap_or(0.00001);

                        let tp_enabled = config_store
                            .get("tp_enabled")
                            .map(|v| v.as_str() == "true")
                            .unwrap_or(false);
                        let sl_enabled = config_store
                            .get("sl_enabled")
                            .map(|v| v.as_str() == "true")
                            .unwrap_or(false);

                        let tp_pct = if tp_enabled {
                            config_store
                                .get("tp_pct")
                                .and_then(|v| v.parse::<f64>().ok())
                        } else {
                            None
                        };

                        let sl_pct = if sl_enabled {
                            config_store
                                .get("sl_pct")
                                .and_then(|v| v.parse::<f64>().ok())
                        } else {
                            None
                        };

                        let dex_type = match src {
                            PairSource::V2 => DexType::V2,
                            PairSource::V3 => DexType::V3,
                            PairSource::FourMeme => DexType::FourMeme,
                            _ => DexType::V2,
                        };

                        if let Some((pnl_pct, first_seen)) = pair_metrics(&pair_addr_str.as_str()) {
                            if pnl_pct < min_pnl_pct
                                && first_seen.elapsed().as_secs() > freshness_secs
                            {
                                return Ok(());
                            }
                        }

                        let submitted = sim_engine.submit_buy(
                            pair_addr_str.clone(),
                            dex_type,
                            pair_info.symbol_base.clone(),
                            pair_info.symbol_quote.clone(),
                            buy_amount,
                            tp_pct,
                            sl_pct,
                        );

                        if submitted {
                            save_log_to_file(&format!(
                                "[sim] ✓ SUBMITTED {} @ {:.8} (buys:{} liq:${:.0})",
                                pair_info.symbol_base,
                                current_price,
                                buy_count,
                                pair_info.liquidity_usd.unwrap_or(0.0)
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

pub async fn auto_trade_real<P>(
    l1: String,
    pair_info: PairInfo,
    buy_count: u32,
    provider: P,
    config_store: &ConfigStore,
    sim_engine: Option<&Arc<tokio::sync::Mutex<SimEngine>>>,
) -> Result<()>
where
    P: Provider + Clone + WalletProvider + Send + Sync + 'static,
{
    if l1.contains("Price: ?") {
        return Ok(());
    }
    let price_opt = extract_price_f64(&l1);
    let src = detect_source(&l1);
    let pair_key = format!("{:#x}", pair_info.pair);

    if let Some(current_price) = price_opt {
        if current_price <= 0.0 {
            return Ok(());
        }

        // First, attempt to exit existing position based on TP/SL/MaxHold
        if let Some(plan) = {
            let mut trader = REAL_TRADER.lock().await;
            trader.sell_decision(&pair_key, current_price, config_store)
        } {
            let res = execute_sell_plan(&plan, provider.clone(), config_store).await;

            {
                let mut trader = REAL_TRADER.lock().await;
                match &res {
                    Ok(_) => trader.finish_sell(&pair_key),
                    Err(e) => {
                        let msg = e.to_string();
                        if msg.contains("TRANSFER_FROM_FAILED")
                            || msg.contains("balance is zero, nothing to sell")
                        {
                            trader.abort_close(&pair_key);
                        } else {
                            trader.abort_close(&pair_key);
                        }
                    }
                }
            }

            if let Some(se_arc) = sim_engine {
                let mut se = se_arc.lock().await;
                if let Some(pos) = se.take_position(&pair_key) {
                    save_log_to_file(&format!(
                        "[trade] mirror close {} ({}) PnL: {:+.6} WBNB",
                        pos.base_token, pos.pair_address, pos.pnl_wbnb
                    ));
                } else {
                    save_log_to_file(&format!(
                        "[trade] mirror close skipped: no sim position {}",
                        pair_key
                    ));
                }
            }

            if let Err(e) = res {
                save_log_to_file(&format!("[trade] sell failed for {}: {}", pair_key, e));
            }
            return Ok(());
        }

        let enabled = config_store
            .get("enabled")
            .map(|v| v.as_str() == "true")
            .unwrap_or(false);
        if !enabled {
            return Ok(());
        }

        let avoid_cn = config_store
            .get("avoid_chinese")
            .map(|v| v.as_str() == "true")
            .unwrap_or(false);
        if avoid_cn
            && (contains_cjk(&pair_info.symbol_base) || contains_cjk(&pair_info.symbol_quote))
        {
            return Ok(());
        }
        if should_avoid_name(&pair_info.symbol_base) || should_avoid_name(&pair_info.symbol_quote) {
            return Ok(());
        }

        let freshness_secs = config_store
            .get("freshness_secs")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(30);
        let min_pnl_pct = config_store
            .get("min_pnl_pct")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(100.0);

        {
            let trader = REAL_TRADER.lock().await;
            if trader.has_position_or_blocked(&pair_key) {
                return Ok(());
            }
            let max_positions = config_store
                .get("max_positions")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(3);
            if trader.positions.len() >= max_positions {
                save_log_to_file(&format!(
                    "[trade] SKIP {}: max positions reached ({}/{})",
                    pair_info.symbol_base,
                    trader.positions.len(),
                    max_positions
                ));
                return Ok(());
            }
        }

        let accepted_quotes_str = config_store
            .get("accepted_quotes")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "BNB,CAKE,USDT,USD1,ASTER,WBNB".to_string());
        let accepted_quotes: Vec<String> = accepted_quotes_str
            .split(',')
            .map(|s| s.trim().to_uppercase())
            .collect();
        if !accepted_quotes.contains(&pair_info.symbol_quote.to_uppercase()) {
            return Ok(());
        }

        if src != PairSource::FourMeme {
            let min_liquidity = config_store
                .get("min_liquidity")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(1000.0);
            let liq_threshold = min_liquidity.max(5.0);
            if let Some(liq) = pair_info.liquidity_usd {
                if liq < liq_threshold {
                    save_log_to_file(&format!(
                        "[trade] REJECTED {}: Liq ${:.0} < min ${:.0}",
                        pair_info.symbol_base, liq, liq_threshold
                    ));
                    return Ok(());
                }
            } else {
                return Ok(());
            }
        }

        if let Some((pnl_pct, first_seen)) = pair_metrics(&pair_key.as_str()) {
            if pnl_pct < min_pnl_pct && first_seen.elapsed().as_secs() > freshness_secs {
                return Ok(());
            }
        }

        // minimum buys in recent window
        let min_buys = config_store
            .get("min_buys")
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(3);
        if buy_count < min_buys {
            save_log_to_file(&format!(
                "[trade] WAITING {}: {} buys < min {}",
                pair_info.symbol_base, buy_count, min_buys
            ));
            return Ok(());
        }

        // dex enablement filter
        let dex_enabled = match src {
            PairSource::V2 => config_store
                .get("dexes")
                .map(|v| v.as_str().contains("v2"))
                .unwrap_or(false),
            PairSource::V3 => config_store
                .get("dexes")
                .map(|v| v.as_str().contains("v3"))
                .unwrap_or(false),
            PairSource::FourMeme => config_store
                .get("dexes")
                .map(|v| v.as_str().contains("fm"))
                .unwrap_or(false),
            _ => false,
        };
        if !dex_enabled {
            return Ok(());
        }

        // amount of BNB (or WBNB) to spend
        let buy_amount_bnb = config_store
            .get("buy_amount_wbnb")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.00001);
        if buy_amount_bnb <= 0.0 {
            return Ok(());
        }
        let amt_str = format!("{}", buy_amount_bnb);
        let amount_wei: U256 = parse_units(amt_str.as_str(), 18)
            .map(Into::into)
            .unwrap_or(U256::ZERO);
        if amount_wei.is_zero() {
            return Ok(());
        }

        // Guard: only buy if PnL >= 100% or pair age <= 30s
        if let Some((pnl_pct, first_seen)) =
            crate::app::pair_streams::pair_metrics(&pair_key.as_str())
        {
            if pnl_pct < 100.0 && first_seen.elapsed() > Duration::from_secs(30) {
                return Ok(());
            }
        }

        // TP/SL settings (mirror sim)
        let tp_enabled = config_store
            .get("tp_enabled")
            .map(|v| v.as_str() == "true")
            .unwrap_or(false);
        let sl_enabled = config_store
            .get("sl_enabled")
            .map(|v| v.as_str() == "true")
            .unwrap_or(false);

        let tp_pct = if tp_enabled {
            config_store
                .get("tp_pct")
                .and_then(|v| v.parse::<f64>().ok())
        } else {
            None
        };

        let sl_pct = if sl_enabled {
            config_store
                .get("sl_pct")
                .and_then(|v| v.parse::<f64>().ok())
        } else {
            None
        };

        let wrap_ratio_pct = wrap_ratio_pct_value(config_store);
        let from = provider.default_signer_address();
        let gas_price_wei = gas_price_wei_u128(config_store);
        let gas_price_wei_override = U256::from(gas_price_wei);

        match src {
            PairSource::V2 => {
                // Only place WBNB->TOKEN if the pair includes WBNB directly
                let token_out: Option<Address> = if pair_info.addr1 == WBNB {
                    Some(pair_info.addr2)
                } else if pair_info.addr2 == WBNB {
                    Some(pair_info.addr1)
                } else {
                    None
                };
                if let Some(token_out) = token_out {
                    if !ensure_wbnb_topup(
                        provider.clone(),
                        from,
                        amount_wei,
                        wrap_ratio_pct,
                        &pair_info.symbol_base,
                    )
                    .await
                    {
                        let mut trader = REAL_TRADER.lock().await;
                        record_buy_failure(&mut trader, &pair_key);
                        return Ok(());
                    }

                    let bal_before = safe_balance_of(provider.clone(), token_out, from).await;

                    let dex_type = DexType::V2;
                    let pancake = PancakeV2::new(provider.clone());
                    let token_out_s = format!("{:#x}", token_out);
                    let (_quoted, tx) = routy_v2::swap_wbnb_to(
                        &pancake,
                        from,
                        token_out_s.as_str(),
                        amount_wei,
                        Some(gas_price_wei),
                    )
                    .await?;

                    tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
                    let bal_after = safe_balance_of(provider.clone(), token_out, from).await;
                    if bal_after <= bal_before {
                        save_log_to_file(&format!(
                            "[trade] ✗ V2 BUY {} tx={} but token balance did not increase (before={} after={})",
                            pair_info.symbol_base, tx, bal_before, bal_after
                        ));
                        return Ok(());
                    }

                    save_log_to_file(&format!(
                        "[trade] ✓ V2 BUY {} via WBNB ({} BNB) tx={} token balance={}",
                        pair_info.symbol_base, buy_amount_bnb, tx, bal_after
                    ));
                    clear_buy_failures(&pair_key);
                    if let Some(se_arc) = sim_engine {
                        let mut se = se_arc.lock().await;
                        let _ = se.submit_buy(
                            pair_key.clone(),
                            dex_type,
                            pair_info.symbol_base.clone(),
                            pair_info.symbol_quote.clone(),
                            buy_amount_bnb,
                            tp_pct,
                            sl_pct,
                        );
                        se.add_real_position(
                            pair_key.clone(),
                            dex_type,
                            pair_info.symbol_base.clone(),
                            pair_info.symbol_quote.clone(),
                            current_price,
                            buy_amount_bnb,
                            tp_pct,
                            sl_pct,
                            pair_info.liquidity_usd,
                        );
                    }
                    let mut trader = REAL_TRADER.lock().await;
                    trader.record_buy(RealPosition {
                        pair_address: pair_key.clone(),
                        dex_type,
                        token_out,
                        base_symbol: pair_info.symbol_base.clone(),
                        entry_price: current_price,
                        buy_amount_bnb,
                        opened_at: Instant::now(),
                    });
                    queue_allowance_job(provider.clone(), dex_type, token_out, gas_price_wei);
                }
            }
            PairSource::V3 => {
                // Only place WBNB->TOKEN if the pair includes WBNB directly
                let token_out: Option<Address> = if pair_info.addr1 == WBNB {
                    Some(pair_info.addr2)
                } else if pair_info.addr2 == WBNB {
                    Some(pair_info.addr1)
                } else {
                    None
                };
                if let Some(token_out) = token_out {
                    if !ensure_wbnb_topup(
                        provider.clone(),
                        from,
                        amount_wei,
                        wrap_ratio_pct,
                        &pair_info.symbol_base,
                    )
                    .await
                    {
                        let mut trader = REAL_TRADER.lock().await;
                        record_buy_failure(&mut trader, &pair_key);
                        return Ok(());
                    }

                    let bal_before = safe_balance_of(provider.clone(), token_out, from).await;

                    let dex_type = DexType::V3;
                    let pancake = PancakeV3::new(provider.clone());
                    let token_out_s = format!("{:#x}", token_out);
                    let (_quoted, tx) = routy_v3::swap_wbnb_to(
                        &pancake,
                        from,
                        token_out_s.as_str(),
                        amount_wei,
                        Some(gas_price_wei),
                    )
                    .await?;
                    tokio::time::sleep(std::time::Duration::from_millis(2000)).await;

                    let bal_after = safe_balance_of(provider.clone(), token_out, from).await;
                    if bal_after <= bal_before {
                        save_log_to_file(&format!(
                            "[trade] ✗ V3 BUY {} tx={} but token balance did not increase (before={} after={})",
                            pair_info.symbol_base, tx, bal_before, bal_after
                        ));
                        return Ok(());
                    }

                    save_log_to_file(&format!(
                        "[trade] ✓ V3 BUY {} via WBNB ({} BNB) tx={} token balance={}",
                        pair_info.symbol_base, buy_amount_bnb, tx, bal_after
                    ));
                    clear_buy_failures(&pair_key);
                    if let Some(se_arc) = sim_engine {
                        let mut se = se_arc.lock().await;
                        let _ = se.submit_buy(
                            pair_key.clone(),
                            dex_type,
                            pair_info.symbol_base.clone(),
                            pair_info.symbol_quote.clone(),
                            buy_amount_bnb,
                            tp_pct,
                            sl_pct,
                        );
                        se.add_real_position(
                            pair_key.clone(),
                            dex_type,
                            pair_info.symbol_base.clone(),
                            pair_info.symbol_quote.clone(),
                            current_price,
                            buy_amount_bnb,
                            tp_pct,
                            sl_pct,
                            pair_info.liquidity_usd,
                        );
                    }
                    let mut trader = REAL_TRADER.lock().await;
                    trader.record_buy(RealPosition {
                        pair_address: pair_key.clone(),
                        dex_type,
                        token_out,
                        base_symbol: pair_info.symbol_base.clone(),
                        entry_price: current_price,
                        buy_amount_bnb,
                        opened_at: Instant::now(),
                    });
                    queue_allowance_job(provider.clone(), dex_type, token_out, gas_price_wei);
                }
            }
            PairSource::FourMeme => {
                // FourMeme uses native BNB; base token is addr1 (pair set to base)
                let slippage_bps: u32 = config_store
                    .get("fm_slippage_bps")
                    .and_then(|v| v.parse::<u32>().ok())
                    .unwrap_or(100);
                let token = pair_info.addr1;
                let router = FmRouter::new(provider.clone());
                let bal_before = safe_balance_of(provider.clone(), token, from).await;

                let buy_res = router
                    .buy_with_bnb_amap(
                        from,
                        token,
                        amount_wei,
                        slippage_bps,
                        None,
                        Some(gas_price_wei_override),
                    )
                    .await;
                let (_est_amount, tx) = match buy_res {
                    Ok(v) => {
                        clear_buy_failures(&pair_key);
                        v
                    }
                    Err(e) => {
                        let mut trader = REAL_TRADER.lock().await;
                        let attempts = record_buy_failure(&mut trader, &pair_key);
                        save_log_to_file(&format!(
                            "[trade] FM BUY failed {} attempts={} err={}",
                            pair_key, attempts, e
                        ));
                        return Ok(());
                    }
                };

                tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
                let bal_after = safe_balance_of(provider.clone(), token, from).await;
                if bal_after <= bal_before {
                    save_log_to_file(&format!(
                        "[trade] ✗ FM BUY {} tx={:?} but token balance did not increase (before={} after={})",
                        pair_info.symbol_base, tx, bal_before, bal_after
                    ));
                    return Ok(());
                }

                save_log_to_file(&format!(
                    "[trade] ✓ FM BUY {} ({} BNB) tx={:?} token balance={}",
                    pair_info.symbol_base, buy_amount_bnb, tx, bal_after
                ));
                if let Some(se_arc) = sim_engine {
                    let mut se = se_arc.lock().await;
                    let _ = se.submit_buy(
                        pair_key.clone(),
                        DexType::FourMeme,
                        pair_info.symbol_base.clone(),
                        pair_info.symbol_quote.clone(),
                        buy_amount_bnb,
                        tp_pct,
                        sl_pct,
                    );
                    se.add_real_position(
                        pair_key.clone(),
                        DexType::FourMeme,
                        pair_info.symbol_base.clone(),
                        pair_info.symbol_quote.clone(),
                        current_price,
                        buy_amount_bnb,
                        tp_pct,
                        sl_pct,
                        pair_info.liquidity_usd,
                    );
                }
                let mut trader = REAL_TRADER.lock().await;
                trader.record_buy(RealPosition {
                    pair_address: pair_key.clone(),
                    dex_type: DexType::FourMeme,
                    token_out: token,
                    base_symbol: pair_info.symbol_base.clone(),
                    entry_price: current_price,
                    buy_amount_bnb,
                    opened_at: Instant::now(),
                });
                queue_allowance_job(provider.clone(), DexType::FourMeme, token, gas_price_wei);
            }
            _ => {}
        }
    }
    Ok(())
}
