use anyhow::Result;

use alloy::primitives::{Address, U256};
use alloy::providers::Provider;

use pancakes::pancake::pancake_swap::{PancakeV3, TxHash};

/// Sell a percentage of an ERC20 into WBNB with sane defaults.
/// Defaults: fee=500 (0.05%), slippage=50 (0.50%), recipient=from, deadline=300s, no sim
pub async fn sell_percent_to_wbnb_simple<P: Provider + Clone>(
    pancake: &PancakeV3<P>,
    from: Address,
    token_in: &str,
    percent_bps: u16,
) -> Result<(U256, TxHash)> {
    pancake
        .sell_percent_to_wbnb(from, token_in, percent_bps, 500, 50, from, 300, false, None)
        .await
}

/// Swap WBNB into a token with sane defaults.
/// Defaults: fee=500 (0.05%), slippage=50 (0.50%), recipient=from, deadline=300s, no sim
pub async fn swap_wbnb_to_token_simple<P: Provider + Clone>(
    pancake: &PancakeV3<P>,
    from: Address,
    token_out: &str,
    bnb_in_amount: U256,
) -> Result<(U256, TxHash)> {
    pancake
        .swap_wbnb_to_token(from, token_out, bnb_in_amount, 500, 50, from, 300, false, None)
        .await
}
