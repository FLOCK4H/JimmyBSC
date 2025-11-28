use anyhow::{bail, Result};
use crate::libs::lookup::save_log_to_file;
use alloy::primitives::{Address, B256, U256};
use alloy::providers::Provider;

use fourmeme::abi::{ITokenManager2, ITokenManagerHelper3};
use fourmeme::addresses::TOKEN_MANAGER_HELPER_3;

alloy::sol! {
    #[sol(rpc)]
    interface IERC20 {
        function allowance(address owner, address spender) view returns (uint256);
        function approve(address spender, uint256 value) returns (bool);
        function balanceOf(address owner) view returns (uint256);
        function decimals() view returns (uint8);
    }
}

pub type TxHash = B256;

#[derive(Clone)]
pub struct FmRouter<P: Provider + Clone> {
    pub provider: P,
}

impl<P: Provider + Clone> FmRouter<P> {
    pub fn new(provider: P) -> Self {
        Self { provider }
    }

    async fn erc20(&self, token: Address) -> IERC20::IERC20Instance<P> {
        IERC20::new(token, self.provider.clone())
    }

    /// - quote == Address::ZERO (BNB pairs): call TokenManager2.buyTokenAMAP (payable)
    /// - quote != Address::ZERO (ERC20 pairs): call Helper3.buyWithEth (payable)
    /// minAmount is derived from Helper3.tryBuy applying negative slippage.
    /// - slippage_bps: e.g. 100 = 1%
    pub async fn buy_with_bnb_amap(
        &self,
        from: Address,
        token: Address,
        funds_wei: U256,
        slippage_bps: u32,
        recipient: Option<Address>,
        gas_price_wei: Option<U256>,
    ) -> Result<(U256, TxHash)> {
        if funds_wei.is_zero() {
            bail!("funds_wei must be > 0");
        }
        save_log_to_file(&format!("Buying with BNB AMAP: token={token:?}"));
        let helper = ITokenManagerHelper3::new(TOKEN_MANAGER_HELPER_3, self.provider.clone());
        let info = helper.getTokenInfo(token).call().await?;
        let token_manager = info.tokenManager;
        if token_manager == Address::ZERO {
            bail!("helper returned zero tokenManager");
        }

        let try_ret = helper.tryBuy(token, U256::ZERO, funds_wei).call().await?;
        let estimated_amount = try_ret.estimatedAmount;
        let min_amount = apply_negative_slippage(estimated_amount, slippage_bps);
        let to = recipient.unwrap_or(from);

        save_log_to_file(&format!(
            "Estimated amount for buy: {:?} (token={token:?})",
            estimated_amount
        ));

        if info.quote == Address::ZERO {
            let tm = ITokenManager2::new(token_manager, self.provider.clone());
            let mut call = tm
                .buyTokenAMAP_0(token, funds_wei, min_amount)
                .from(from)
                .value(funds_wei);
    
            if let Some(gp) = gas_price_wei {
                call = call.gas_price(gp.try_into().unwrap());
            }
    
            let pending = call.send().await?;
            let tx = *pending.tx_hash();
            let _ = pending.get_receipt().await;
            Ok((estimated_amount, tx))
        } else {
            let mut call = helper
                .buyWithEth(U256::ZERO, token, to, funds_wei, min_amount)
                .from(from)
                .value(funds_wei);
    
            if let Some(gp) = gas_price_wei {
                call = call.gas_price(gp.try_into().unwrap());
            }
    
            let pending = call.send().await?;
            let tx = *pending.tx_hash();
            let _ = pending.get_receipt().await;
            Ok((estimated_amount, tx))
        }
    }

    /// Sell a percentage of holder's balance for BNB.
    /// Path based on quote token:
    /// - quote == Address::ZERO: call TM2.sellToken(token, amount)
    /// - quote != Address::ZERO: call Helper3.sellForEth with minFunds derived from trySell
    pub async fn sell_percent(
        &self,
        from: Address,
        token: Address,
        percent_bps: u32, // 1..=10000
        gas_price_wei: Option<U256>,
    ) -> Result<(U256, TxHash)> {
        if percent_bps == 0 || percent_bps > 10_000 {
            bail!("percent_bps must be 1..=10000");
        }
        save_log_to_file(&format!("Selling percent: token={token:?}"));
        let helper = ITokenManagerHelper3::new(TOKEN_MANAGER_HELPER_3, self.provider.clone());
        let info = helper.getTokenInfo(token).call().await?;
        let token_manager = info.tokenManager;
        if token_manager == Address::ZERO {
            bail!("helper returned zero tokenManager");
        }

        let erc20 = self.erc20(token).await;
        let bal = erc20.balanceOf(from).call().await?;
        if bal.is_zero() {
            bail!("balance is zero, nothing to sell");
        }
        let raw_amount = bal * U256::from(percent_bps) / U256::from(10_000u64);
        let amount = align_to_gwei(raw_amount);
        if amount.is_zero() {
            bail!("computed sell amount is zero after gwei alignment");
        }

        let est = helper.trySell(token, amount).call().await.ok();
        let est_funds = est.as_ref().map(|r| r.funds).unwrap_or(U256::ZERO);

        if info.quote == Address::ZERO {
            let tm = ITokenManager2::new(token_manager, self.provider.clone());
            let mut call = tm.sellToken(token, amount).from(from);
            if let Some(gp) = gas_price_wei {
                call = call.gas_price(gp.try_into().unwrap());
            }
            let pending = call.send().await?;
            let tx = *pending.tx_hash();
            let _ = pending.get_receipt().await;
            Ok((est_funds, tx))
        } else {
            let min_funds = align_to_gwei(apply_negative_slippage(est_funds, 100)); // 1% negative slippage default + gwei alignment
            let mut call = helper
                .sellForEth(
                    U256::ZERO,
                    token,
                    amount,
                    min_funds,
                    U256::ZERO,
                    Address::ZERO,
                )
                .from(from);
            if let Some(gp) = gas_price_wei {
                call = call.gas_price(gp.try_into().unwrap());
            }
            let pending = match call.send().await {
                Ok(p) => p,
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("Slippage") || msg.contains("slippage") {
                        let mut retry = helper
                            .sellForEth(
                                U256::ZERO,
                                token,
                                amount,
                                U256::ZERO,
                                U256::ZERO,
                                Address::ZERO,
                            )
                            .from(from);
                        if let Some(gp) = gas_price_wei {
                            retry = retry.gas_price(gp.try_into().unwrap());
                        }
                        retry.send().await?
                    } else {
                        return Err(e.into());
                    }
                }
            };

            let tx = *pending.tx_hash();
            let _ = pending.get_receipt().await;
            Ok((est_funds, tx))
        }
    }

    pub async fn sell_percent_pct(
        &self,
        from: Address,
        token: Address,
        percent_points: u32, // 1..=100
        gas_price_wei: Option<U256>,
    ) -> Result<(U256, TxHash)> {
        if percent_points == 0 || percent_points > 100 {
            bail!("percent must be 1..=100");
        }
        let bps = percent_points.saturating_mul(100);
        self.sell_percent(from, token, bps, gas_price_wei).await
    }
}

fn apply_negative_slippage(amount: U256, bps: u32) -> U256 {
    if amount.is_zero() || bps == 0 {
        return amount;
    }
    let num = amount.saturating_mul(U256::from(10_000u64 - bps as u64));
    num / U256::from(10_000u64)
}

fn align_to_gwei(amount: U256) -> U256 {
    let gwei = U256::from(1_000_000_000u64);
    if amount < gwei {
        return U256::ZERO;
    }
    (amount / gwei) * gwei
}
