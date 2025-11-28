use anyhow::Result;

use alloy::primitives::{Address, U256};
use alloy::providers::Provider;

use pancakes::pancake::pancake_swap::{PancakeV3, TxHash};

/// Sell a percentage of an ERC20 into WBNB with sane defaults (v3).
/// Defaults: fee=500 (0.05%), slippage=50 (0.50%), recipient=from, deadline=300s, no sim
pub async fn sell_pct_to_wbnb<P: Provider + Clone>(
    pancake: &PancakeV3<P>,
    from: Address,
    token_in: &str,
    percent_bps: u16,
    gas_price_wei: Option<u128>,
) -> Result<(U256, TxHash)> {
    pancake
        .sell_percent_to_wbnb(
            from,
            token_in,
            percent_bps,
            500,
            50,
            from,
            300,
            false,
            gas_price_wei,
        )
        .await
}

/// Swap WBNB into a token with sane defaults (v3).
/// Defaults: fee=500 (0.05%), slippage=50 (0.50%), recipient=from, deadline=300s, no sim
pub async fn swap_wbnb_to<P: Provider + Clone>(
    pancake: &PancakeV3<P>,
    from: Address,
    token_out: &str,
    bnb_in_amount: U256,
    gas_price_wei: Option<u128>,
) -> Result<(U256, TxHash)> {
    pancake
        .swap_wbnb_to_token(
            from,
            token_out,
            bnb_in_amount,
            500,
            50,
            from,
            300,
            false,
            gas_price_wei,
        )
        .await
}

/// Swap token -> token (v3) with sane defaults.
/// Defaults: fee=500 (0.05%), slippage=50 (0.50%), recipient=from, deadline=300s, no sim
pub async fn swap_token_to<P: Provider + Clone>(
    pancake: &PancakeV3<P>,
    from: Address,
    token_in: &str,
    token_out: &str,
    amount_in: U256,
    gas_price_wei: Option<u128>,
) -> Result<(U256, TxHash)> {
    pancake
        .swap_token_to_token(
            from,
            token_in,
            token_out,
            amount_in,
            500,
            50,
            from,
            300,
            false,
            gas_price_wei,
        )
        .await
}
