use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Result, bail};

use alloy::primitives::{Address, U256, B256};
use alloy::providers::Provider;

use crate::pancake::pancake_swap_v2::path::apply_slippage_bps;
use crate::{log};
use crate::writing::cc;
use crate::pancake::pancake_swap_v2::IERC20::IERC20Instance;
use crate::pancake::pancake_swap_v2::addresses::{PANCAKE_V2_ROUTER, PANCAKE_V2_FACTORY, WBNB};
pub type TxHash = B256;

fn gas_price_override(gas_price_wei: Option<u128>) -> Option<u128> {
    gas_price_wei.filter(|g| *g > 0)
}

pub fn format_token(amount: U256, decimals: u32) -> String {
    let base = U256::from(10u64).pow(U256::from(decimals));
    let whole = amount / base;
    let frac  = amount % base;
    if frac.is_zero() {
        return format!("{whole}");
    }
    let mut frac_str = format!("{:0width$}", frac, width = decimals as usize);
    while frac_str.ends_with('0') {
        frac_str.pop();
    }
    format!("{whole}.{frac_str}")
}

alloy::sol! {
    #[sol(rpc)]
    interface IPancakeRouter02 {
        function WETH() pure returns (address);
        function factory() pure returns (address);

        function getAmountsOut(uint amountIn, address[] calldata path) external view returns (uint[] memory amounts);
        function getAmountsIn(uint amountOut, address[] calldata path) external view returns (uint[] memory amounts);

        function swapExactTokensForTokens(
            uint amountIn,
            uint amountOutMin,
            address[] calldata path,
            address to,
            uint deadline
        ) external returns (uint[] memory amounts);
    }

    #[sol(rpc)]
    interface IERC20 {
        function allowance(address owner, address spender) view returns (uint256);
        function approve(address spender, uint256 value) returns (bool);
        function balanceOf(address owner) view returns (uint256);
        function decimals() view returns (uint8);
    }
}

#[derive(Clone)]
pub struct PancakeV2<P: Provider + Clone> {
    pub provider: P,
    pub router_addr: Address,
    pub factory_addr: Address,
}

impl<P: Provider + Clone> PancakeV2<P> {
    pub fn new(provider: P) -> Self {
        Self {
            provider,
            router_addr: PANCAKE_V2_ROUTER,
            factory_addr: PANCAKE_V2_FACTORY,
        }
    }

    async fn erc20(&self, token: Address) -> IERC20Instance<P> {
        IERC20::new(token, self.provider.clone())
    }

    async fn token_decimals(&self, token: Address) -> Result<u32> {
        let erc20 = self.erc20(token).await;
        Ok(erc20.decimals().call().await? as u32)
    }

    async fn token_balance_of(&self, owner: Address, token: Address) -> Result<U256> {
        let erc20 = self.erc20(token).await;
        Ok(erc20.balanceOf(owner).call().await?)
    }

    async fn approve_if_needed(&self, token: Address, from: Address, needed: U256) -> Result<()> {
        let erc20 = self.erc20(token).await;
        let allowance = erc20.allowance(from, self.router_addr).call().await?;
        if allowance >= needed {
            log!(cc::YELLOW, "Approval not needed (allowance >= amount)");
            return Ok(());
        }
        let pending = erc20.approve(self.router_addr, U256::MAX).from(from).send().await?;
        let tx = *pending.tx_hash();
        let _ = pending.get_receipt().await;
        log!(cc::YELLOW, "Approved {:?} for router {:?} in tx {:?}", token, self.router_addr, tx);
        Ok(())
    }

    async fn quote_exact_in(&self, token_in: Address, token_out: Address, amount_in: U256) -> Result<U256> {
        let router = IPancakeRouter02::new(self.router_addr, self.provider.clone());
        let path = vec![token_in, token_out];
        let amounts: Vec<U256> = router.getAmountsOut(amount_in, path).call().await?;
        amounts
            .last()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("router.getAmountsOut returned empty amounts"))
    }

    pub async fn sell_percent_to_wbnb(
        &self,
        from: Address,
        token_in_str: &str,    // e.g. "0x7130..." (BTCB)
        percent_bps: u16,      // 1..=10000 → 10000 = 100%
        fee: u32,              // v2 has no fee param; kept for API parity
        slippage_bps: u32,     // e.g. 50 = 0.5%
        recipient: Address,
        deadline_secs_from_now: u64,
        try_sim: bool,
        gas_price_wei: Option<u128>,
    ) -> Result<(U256, TxHash)> {
        if percent_bps == 0 || percent_bps > 10_000 {
            bail!("percent_bps must be 1..=10000");
        }

        let _ = fee; // v2 path fees are implicit in pool, param kept for symmetry

        let token_in = Address::from_str(token_in_str)
            .map_err(|e| anyhow::anyhow!("bad token address `{}`: {e}", token_in_str))?;
        log!(cc::YELLOW, "Token {} -> WBNB (v2)", token_in);

        let decs = self.token_decimals(token_in).await?;
        log!(cc::YELLOW, "Token {} has {} decimals", token_in, decs);

        let balance = self.token_balance_of(from, token_in).await?;
        log!(cc::YELLOW, "On-chain balance: {}", format_token(balance, decs));
        if balance.is_zero() {
            bail!("balance is zero, nothing to sell");
        }

        let amount_in = balance * U256::from(percent_bps) / U256::from(10_000u64);
        log!(cc::YELLOW, "We will sell {} / 10000 = {} (raw)", percent_bps, amount_in);
        log!(cc::YELLOW, "Human sell amount: {}", format_token(amount_in, decs));

        if amount_in.is_zero() {
            bail!("amount after applying percent is zero – token balance too small");
        }

        let quoted = self.quote_exact_in(token_in, WBNB, amount_in).await?;
        log!(cc::YELLOW, "Quoted out: {} WBNB", format_token(quoted, 18));

        let min_out = apply_slippage_bps(quoted, slippage_bps);
        log!(cc::YELLOW, "Minimum out: {} WBNB", format_token(min_out, 18));

        let deadline = U256::from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + deadline_secs_from_now,
        );

        let router = IPancakeRouter02::new(self.router_addr, self.provider.clone());
        let path = vec![token_in, WBNB];

        if try_sim {
            match router
                .swapExactTokensForTokens(amount_in, min_out, path.clone(), recipient, deadline)
                .from(from)
                .call()
                .await
            {
                Ok(amounts) => {
                    if let Some(out) = amounts.last() {
                        log!(cc::YELLOW, "Simulation OK, out = {}", out);
                    } else {
                        log!(cc::YELLOW, "Simulation OK, empty amounts vector");
                    }
                }
                Err(e) => {
                    log!(cc::YELLOW, "Simulation failed on BSC (often normal) {}", e);
                }
            }
        }

        let mut call = router
            .swapExactTokensForTokens(amount_in, min_out, path, recipient, deadline)
            .from(from);
        if let Some(gas_price) = gas_price_override(gas_price_wei) {
            call = call.gas_price(gas_price);
        }
        let pending = call.send().await?;
        let tx = *pending.tx_hash();
        
        let _ = pending.get_receipt().await;
        Ok((quoted, tx))
    }

    pub async fn swap_wbnb_to_token(
        &self,
        from: Address,
        token_out_str: &str,  // e.g. "0x7130..." (BTCB)
        bnb_in_amount: U256,  // WBNB amount to spend
        fee: u32,             // v2 has no fee param; kept for API parity
        slippage_bps: u32,    // e.g. 50 = 0.5%
        recipient: Address,
        deadline_secs_from_now: u64,
        try_sim: bool,
        gas_price_wei: Option<u128>,
    ) -> Result<(U256, TxHash)> {
        let _ = fee; // unused in v2

        let token_out = Address::from_str(token_out_str)
            .map_err(|e| anyhow::anyhow!("bad token address `{}`: {e}", token_out_str))?;
        log!(cc::YELLOW, "WBNB -> Token {} (v2)", token_out);

        let decs = self.token_decimals(token_out).await?;
        log!(cc::YELLOW, "Token {token_out} has {decs} decimals");

        let balance = self.token_balance_of(from, WBNB).await?;
        log!(cc::YELLOW, "On-chain balance: {} WBNB", format_token(balance, 18));
        if balance.is_zero() {
            bail!("balance is zero, nothing to swap");
        }

        log!(cc::YELLOW, "We will spend {} WBNB", format_token(bnb_in_amount, 18));

        if bnb_in_amount.is_zero() {
            bail!("bnb in amount is zero – wbnb balance too small");
        }

        let quoted = self.quote_exact_in(WBNB, token_out, bnb_in_amount).await?;
        log!(cc::YELLOW, "Quoted out: {} tokens", format_token(quoted, decs));

        let min_out = apply_slippage_bps(quoted, slippage_bps);
        log!(cc::YELLOW, "Minimum out: {} tokens", format_token(min_out, decs));

        self.approve_if_needed(WBNB, from, bnb_in_amount).await?;
        log!(cc::YELLOW, "Approved WBNB for router {:?}", self.router_addr);

        let deadline = U256::from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + deadline_secs_from_now,
        );
        log!(cc::YELLOW, "Deadline: {:?}", deadline);

        let router = IPancakeRouter02::new(self.router_addr, self.provider.clone());
        let path = vec![WBNB, token_out];

        if try_sim {
            match router
                .swapExactTokensForTokens(bnb_in_amount, min_out, path.clone(), recipient, deadline)
                .from(from)
                .call()
                .await
            {
                Ok(amounts) => {
                    if let Some(out) = amounts.last() {
                        log!(cc::YELLOW, "Simulation OK, out = {}", out);
                    } else {
                        log!(cc::YELLOW, "Simulation OK, empty amounts vector");
                    }
                }
                Err(e) => {
                    log!(cc::YELLOW, "Simulation failed on BSC (often normal) {}", e);
                }
            }
        }

        let mut call = router
            .swapExactTokensForTokens(bnb_in_amount, min_out, path, recipient, deadline)
            .from(from);
        if let Some(gas_price) = gas_price_override(gas_price_wei) {
            call = call.gas_price(gas_price);
        }
        log!(cc::YELLOW, "Sending WBNB -> {} tx", token_out_str);
        let pending = call.send().await?;
        let tx = *pending.tx_hash();
        log!(cc::YELLOW, "WBNB -> {} Tx: {:?}", token_out_str, tx);
        let receipt = pending.get_receipt().await;
        if let Ok(receipt) = receipt {
            log!(cc::YELLOW, "Receipt: {:?}", receipt);
        } else {
            log!(cc::YELLOW, "Receipt error: {:?}", receipt);
        }
        Ok((quoted, tx))
    }

    pub async fn swap_token_to_token(
        &self,
        from: Address,
        token_in_str: &str,
        token_out_str: &str,
        amount_in: U256,
        fee: u32,
        slippage_bps: u32,
        recipient: Address,
        deadline_secs_from_now: u64,
        try_sim: bool,
        gas_price_wei: Option<u128>,
    ) -> Result<(U256, TxHash)> {
        let _ = fee; // unused in v2

        let token_in = Address::from_str(token_in_str)
            .map_err(|e| anyhow::anyhow!("bad token address `{}`: {e}", token_in_str))?;
        let token_out = Address::from_str(token_out_str)
            .map_err(|e| anyhow::anyhow!("bad token address `{}`: {e}", token_out_str))?;
        log!(cc::YELLOW, "Token {} -> Token {} (v2)", token_in, token_out);

        let out_decs = self.token_decimals(token_out).await?;
        log!(cc::YELLOW, "Out token {token_out} has {out_decs} decimals");

        if amount_in.is_zero() {
            bail!("amount_in is zero – nothing to swap");
        }

        let quoted = self.quote_exact_in(token_in, token_out, amount_in).await?;
        log!(cc::YELLOW, "Quoted out: {} tokens", format_token(quoted, out_decs));

        let min_out = apply_slippage_bps(quoted, slippage_bps);
        log!(cc::YELLOW, "Minimum out: {} tokens", format_token(min_out, out_decs));

        self.approve_if_needed(token_in, from, amount_in).await?;

        let deadline = U256::from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + deadline_secs_from_now,
        );

        let router = IPancakeRouter02::new(self.router_addr, self.provider.clone());
        let path = vec![token_in, token_out];

        if try_sim {
            match router
                .swapExactTokensForTokens(amount_in, min_out, path.clone(), recipient, deadline)
                .from(from)
                .call()
                .await
            {
                Ok(amounts) => {
                    if let Some(out) = amounts.last() {
                        log!(cc::YELLOW, "Simulation OK, out = {}", out);
                    } else {
                        log!(cc::YELLOW, "Simulation OK, empty amounts vector");
                    }
                }
                Err(e) => {
                    log!(cc::YELLOW, "Simulation failed on BSC (often normal) {}", e);
                }
            }
        }

        let mut call = router
            .swapExactTokensForTokens(amount_in, min_out, path, recipient, deadline)
            .from(from);
        if let Some(gas_price) = gas_price_override(gas_price_wei) {
            call = call.gas_price(gas_price);
        }
        let pending = call.send().await?;
        let tx = *pending.tx_hash();
        let _ = pending.get_receipt().await;
        Ok((quoted, tx))
    }
}
