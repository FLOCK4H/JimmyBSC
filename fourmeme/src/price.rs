use anyhow::Result;
use alloy::primitives::{Address, U256};
use alloy::providers::Provider;

use crate::addresses::TOKEN_MANAGER_HELPER_3;
use crate::abi::ITokenManagerHelper3;

#[derive(Debug, Clone)]
pub struct TokenInfo {
    pub version: u64,
    pub token_manager: Address,
    pub quote: Address,
    pub last_price: U256,
    pub liquidity_added: bool,
}

pub async fn get_token_info<P: Provider + Clone>(provider: P, token: Address) -> Result<TokenInfo> {
    let helper = ITokenManagerHelper3::new(TOKEN_MANAGER_HELPER_3, provider);
    let ret = helper.getTokenInfo(token).call().await?;
    Ok(TokenInfo {
        version: ret.version.try_into().unwrap_or(0u64),
        token_manager: ret.tokenManager,
        quote: ret.quote,
        last_price: ret.lastPrice,
        liquidity_added: ret.liquidityAdded,
    })
}

pub fn format_units(amount: U256, decimals: u32) -> String {
    if amount.is_zero() { return "0".into(); }
    if amount > U256::from(u128::MAX) {
        return format!("{amount}");
    }
    let v: u128 = amount.try_into().unwrap();
    let scale = 10u128.saturating_pow(decimals.min(38) as u32);
    let whole = v / scale;
    let frac = v % scale;
    if frac == 0 {
        format!("{whole}")
    } else {
        let mut frac_str = format!("{:0width$}", frac, width = decimals as usize);
        while frac_str.ends_with('0') { frac_str.pop(); }
        format!("{whole}.{frac_str}")
    }
}


