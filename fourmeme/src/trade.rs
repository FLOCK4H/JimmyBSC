use anyhow::Result;
use alloy::primitives::{U256, Address};
use alloy::providers::Provider;

use crate::addresses::TOKEN_MANAGER_HELPER_3;
use crate::abi::{ITokenManagerHelper3, IERC20Meta};

#[derive(Debug, Clone)]
pub struct BuyQuote {
    pub token_manager: Address,
    pub quote: Address,
    pub estimated_amount: U256,
    pub estimated_cost: U256,
    pub estimated_fee: U256,
    pub amount_msg_value: U256,
    pub amount_approval: U256,
    pub amount_funds: U256,
}

pub async fn try_buy<P: Provider + Clone>(provider: P, token: Address, amount: U256, funds: U256) -> Result<BuyQuote> {
    let helper = ITokenManagerHelper3::new(TOKEN_MANAGER_HELPER_3, provider);
    let ret = helper.tryBuy(token, amount, funds).call().await?;
    Ok(BuyQuote {
        token_manager: ret.tokenManager,
        quote: ret.quote,
        estimated_amount: ret.estimatedAmount,
        estimated_cost: ret.estimatedCost,
        estimated_fee: ret.estimatedFee,
        amount_msg_value: ret.amountMsgValue,
        amount_approval: ret.amountApproval,
        amount_funds: ret.amountFunds,
    })
}

#[derive(Debug, Clone)]
pub struct SellQuote {
    pub token_manager: Address,
    pub quote: Address,
    pub funds: U256,
    pub fee: U256,
}

pub async fn try_sell<P: Provider + Clone>(provider: P, token: Address, amount: U256) -> Result<SellQuote> {
    let helper = ITokenManagerHelper3::new(TOKEN_MANAGER_HELPER_3, provider);
    let ret = helper.trySell(token, amount).call().await?;
    Ok(SellQuote { token_manager: ret.tokenManager, quote: ret.quote, funds: ret.funds, fee: ret.fee })
}

pub async fn try_sell_pct<P: Provider + Clone>(provider: P, token: Address, holder: Address, percent_bips: u32) -> Result<SellQuote> {
    let bal = IERC20Meta::new(token, provider.clone()).balanceOf(holder).call().await?;
    let amount = bal * U256::from(percent_bips) / U256::from(10_000u64);
    try_sell(provider, token, amount).await
}


