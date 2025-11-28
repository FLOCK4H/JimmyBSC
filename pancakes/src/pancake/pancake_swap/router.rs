use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Result, bail};

use alloy::primitives::{Address, U256, B256};
use alloy::primitives::aliases::{U24, U160};
use alloy::providers::Provider;

use crate::pancake::pancake_swap::addresses::{PANCAKE_V3_QUOTER_V2, PANCAKE_V3_SWAP_ROUTER, PANCAKE_SMART_ROUTER, WBNB};
use crate::pancake::pancake_swap::IERC20::IERC20Instance;
use crate::pancake::pancake_swap::path::apply_slippage_bps;
use crate::{log};
use crate::writing::cc;

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
    interface IV3SwapRouter {
        struct ExactInputSingleParams {
            address tokenIn;
            address tokenOut;
            uint24  fee;
            address recipient;
            uint256 deadline;
            uint256 amountIn;
            uint256 amountOutMinimum;
            uint160 sqrtPriceLimitX96;
        }
        function exactInputSingle(ExactInputSingleParams calldata params)
            payable
            returns (uint256 amountOut);
    }

    #[sol(rpc)]
    interface IQuoterV2 {
        struct QuoteExactInputSingleParams {
            address tokenIn;
            address tokenOut;
            uint256 amountIn;
            uint24  fee;
            uint160 sqrtPriceLimitX96;
        }
        function quoteExactInputSingle(QuoteExactInputSingleParams calldata params)
            returns (uint256 amountOut, uint160, uint32, uint256);
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
pub struct PancakeV3<P: Provider + Clone> {
    pub provider: P,
    pub router_addr: Address,
    pub quoter_addr: Address,
    pub smart_router_addr: Address,
}

impl<P: Provider + Clone> PancakeV3<P> {
    pub fn new(provider: P) -> Self {
        Self {
            provider,
            router_addr: PANCAKE_V3_SWAP_ROUTER,
            quoter_addr: PANCAKE_V3_QUOTER_V2,
            smart_router_addr: PANCAKE_SMART_ROUTER,
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
                tokio::spawn(async move {
            let _ = pending.get_receipt().await;
        });
        log!(cc::YELLOW, "Approved {:?} for router {:?} in tx {:?}", token, self.router_addr, tx);
        Ok(())
    }

    async fn quote_exact_in_single(
        &self,
        token_in: Address,
        token_out: Address,
        fee: u32,
        amount_in: U256,
    ) -> Result<U256> {
        let quoter = IQuoterV2::new(self.quoter_addr, self.provider.clone());
        let params = IQuoterV2::QuoteExactInputSingleParams {
            tokenIn: token_in,
            tokenOut: token_out,
            amountIn: amount_in,
            fee: U24::from(fee),
            sqrtPriceLimitX96: U160::ZERO,
        };
        let ret = quoter.quoteExactInputSingle(params).call().await?;
        Ok(ret.amountOut)
    }

    pub async fn sell_percent_to_wbnb(
        &self,
        from: Address,
        token_in_str: &str,    // e.g. "0x7130..." (BTCB)
        percent_bps: u16,      // 1..=10000 → 10000 = 100%
        fee: u32,              // 100 / 500 / 800 / 2500 / 10000
        slippage_bps: u32,     // e.g. 50 = 0.5%
        recipient: Address,
        deadline_secs_from_now: u64,
        try_sim: bool,
        gas_price_wei: Option<u128>,
    ) -> Result<(U256, TxHash)> {
        if percent_bps == 0 || percent_bps > 10_000 {
            bail!("percent_bps must be 1..=10000");
        }

        let token_in = Address::from_str(token_in_str)
            .map_err(|e| anyhow::anyhow!("bad token address `{}`: {e}", token_in_str))?;
        log!(cc::YELLOW, "Token {} -> WBNB", token_in);

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

        let quoted = self
            .quote_exact_in_single(token_in, WBNB, fee, amount_in)
            .await?;
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

        let params = IV3SwapRouter::ExactInputSingleParams {
            tokenIn: token_in,
            tokenOut: WBNB,
            fee: U24::from(fee),
            recipient,
            deadline,
            amountIn: amount_in,
            amountOutMinimum: min_out,
            sqrtPriceLimitX96: U160::ZERO,
        };

        let router = IV3SwapRouter::new(self.router_addr, self.provider.clone());

        if try_sim {
            match router.exactInputSingle(params.clone()).from(from).call().await {
                Ok(sim_out) => {
                    log!(cc::YELLOW, "Simulation OK, out = {}", sim_out);
                }
                Err(e) => {
                    // THIS is the one you saw:
                    log!(cc::YELLOW, "Simulation failed on BSC (this is normal) {}", e);
                    // do not return here
                }
            }
        }

        let mut call = router.exactInputSingle(params).from(from);
        if let Some(gas_price) = gas_price_override(gas_price_wei) {
            call = call.gas_price(gas_price);
        }
        let pending = call.send().await?;
        let tx = *pending.tx_hash();
                tokio::spawn(async move {
            let _ = pending.get_receipt().await;
        });
        Ok((quoted, tx))
    }

    pub async fn swap_wbnb_to_token(
        &self,
        from: Address,
        token_out_str: &str,    // e.g. "0x7130..." (BTCB)
        bnb_in_amount: U256,      // 1..=10000 → 10000 = 100%
        fee: u32,              // 100 / 500 / 800 / 2500 / 10000
        slippage_bps: u32,     // e.g. 50 = 0.5%
        recipient: Address,
        deadline_secs_from_now: u64,
        try_sim: bool,
        gas_price_wei: Option<u128>,
    ) -> Result<(U256, TxHash)> {

        let token_out = Address::from_str(token_out_str)
            .map_err(|e| anyhow::anyhow!("bad token address `{}`: {e}", token_out_str))?;
        log!(cc::YELLOW, "WBNB -> Token {}", token_out);

        let decs = self.token_decimals(token_out).await?;
        log!(cc::YELLOW, "Token {token_out} has {decs} decimals");

        let balance = self.token_balance_of(from, WBNB).await?;
        log!(cc::YELLOW, "On-chain balance: {} BNB", format_token(balance, 18));
        if balance.is_zero() {
            bail!("balance is zero, nothing to swap");
        }

        log!(cc::YELLOW, "We will spend {} BNB", format_token(bnb_in_amount, 18));

        if bnb_in_amount.is_zero() {
            bail!("bnb in amount is zero – bnb balance too small");
        }

        let quoted = self
            .quote_exact_in_single(WBNB, token_out, fee, bnb_in_amount)
            .await?;
        log!(cc::YELLOW, "Quoted out: {} tokens", format_token(quoted, decs));

        let min_out = apply_slippage_bps(quoted, slippage_bps);
        log!(cc::YELLOW, "Minimum out: {} tokens", format_token(min_out, decs));

        self.approve_if_needed(WBNB, from, bnb_in_amount).await?;

        let deadline = U256::from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + deadline_secs_from_now,
        );

        let params = IV3SwapRouter::ExactInputSingleParams {
            tokenIn: WBNB,
            tokenOut: token_out,
            fee: U24::from(fee),
            recipient,
            deadline,
            amountIn: bnb_in_amount,
            amountOutMinimum: min_out,
            sqrtPriceLimitX96: U160::ZERO,
        };

        let router = IV3SwapRouter::new(self.router_addr, self.provider.clone());

        if try_sim {
            match router.exactInputSingle(params.clone()).from(from).call().await {
                Ok(sim_out) => {
                    log!(cc::YELLOW, "Simulation OK, out = {}", sim_out);
                }
                Err(e) => {
                    // THIS is the one you saw:
                    log!(cc::YELLOW, "Simulation failed on BSC (this is normal) {}", e);
                    // do not return here
                }
            }
        }

        let mut call = router.exactInputSingle(params).from(from);
        if let Some(gas_price) = gas_price_override(gas_price_wei) {
            call = call.gas_price(gas_price);
        }
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
        fee: u32,              // pool fee tier
        slippage_bps: u32,     // e.g. 50 = 0.5%
        recipient: Address,
        deadline_secs_from_now: u64,
        try_sim: bool,
        gas_price_wei: Option<u128>,
    ) -> Result<(U256, TxHash)> {
        let token_in = Address::from_str(token_in_str)
            .map_err(|e| anyhow::anyhow!("bad token address `{}`: {e}", token_in_str))?;
        let token_out = Address::from_str(token_out_str)
            .map_err(|e| anyhow::anyhow!("bad token address `{}`: {e}", token_out_str))?;
        log!(cc::YELLOW, "Token {} -> Token {}", token_in, token_out);

        let out_decs = self.token_decimals(token_out).await?;
        log!(cc::YELLOW, "Out token {} has {} decimals", token_out, out_decs);

        if amount_in.is_zero() {
            bail!("amount_in is zero – nothing to swap");
        }

        let quoted = self
            .quote_exact_in_single(token_in, token_out, fee, amount_in)
            .await?;
        log!(cc::YELLOW, "Quoted out: {} tokens", format_token(quoted, out_decs));

        let min_out = apply_slippage_bps(quoted, slippage_bps);
        log!(cc::YELLOW, "Minimum out: {} tokens", format_token(min_out, out_decs));

        self.approve_if_needed(token_in, from, amount_in).await?;
        log!(cc::YELLOW, "Approved {:?} for router", token_in);

        let deadline = U256::from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + deadline_secs_from_now,
        );

        let params = IV3SwapRouter::ExactInputSingleParams {
            tokenIn: token_in,
            tokenOut: token_out,
            fee: U24::from(fee),
            recipient,
            deadline,
            amountIn: amount_in,
            amountOutMinimum: min_out,
            sqrtPriceLimitX96: U160::ZERO,
        };

        let router = IV3SwapRouter::new(self.router_addr, self.provider.clone());

        if try_sim {
            match router.exactInputSingle(params.clone()).from(from).call().await {
                Ok(sim_out) => {
                    log!(cc::YELLOW, "Simulation OK, out = {}", sim_out);
                }
                Err(e) => {
                    log!(cc::YELLOW, "Simulation failed on BSC (this is normal) {}", e);
                }
            }
        }

        let mut call = router.exactInputSingle(params).from(from);
        if let Some(gas_price) = gas_price_override(gas_price_wei) {
            call = call.gas_price(gas_price);
        }
        let pending = call.send().await?;
        let tx = *pending.tx_hash();
                tokio::spawn(async move {
            let _ = pending.get_receipt().await;
        });
        Ok((quoted, tx))
    }
}
