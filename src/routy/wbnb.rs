use anyhow::Result;

use alloy::primitives::{Address, U256};
use alloy::providers::Provider;

use pancakes::pancake::pancake_swap::addresses::WBNB;
use pancakes::pancake::pancake_swap::TxHash;

alloy::sol! {
    #[sol(rpc)]
    interface IWBNB {
        function deposit() payable;
        function withdraw(uint256 wad);
        function balanceOf(address owner) view returns (uint256);
    }
}

/// Wrap native BNB into WBNB by calling WBNB.deposit with `amount` as msg.value
pub async fn wrap_bnb<P: Provider + Clone>(
    provider: P,
    from: Address,
    amount: U256,
) -> Result<TxHash> {
    let wbnb = IWBNB::new(WBNB, provider.clone());
    let pending = wbnb.deposit().from(from).value(amount).send().await?;
    let tx = *pending.tx_hash();
    tokio::spawn(async move {
        let _ = pending.get_receipt().await;
    });
    Ok(tx)
}

/// Unwrap WBNB back to native BNB by calling WBNB.withdraw(amount)
pub async fn unwrap_wbnb<P: Provider + Clone>(
    provider: P,
    from: Address,
    amount: U256,
) -> Result<TxHash> {
    let wbnb = IWBNB::new(WBNB, provider.clone());
    let pending = wbnb.withdraw(amount).from(from).send().await?;
    let tx = *pending.tx_hash();
    tokio::spawn(async move {
        let _ = pending.get_receipt().await;
    });
    Ok(tx)
}
