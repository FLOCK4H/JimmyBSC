use anyhow::Result;

use alloy::primitives::{keccak256, Address, B256};
use alloy::providers::Provider;
use alloy::rpc::types::eth::Filter;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use alloy::primitives::aliases::U24;
use pancakes::pancake::pancake_swap::addresses::PANCAKE_V3_FACTORY;
use pancakes::pancake::pancake_swap_v2::addresses::PANCAKE_V2_FACTORY;
use pancakes::plug::price::IPancakeV3FactoryView;
use pancakes::plug::price::{get_price_v2, get_price_v3, PriceQuote};
use pancakes::plug::v2::IPancakeV2FactoryView;

use crate::libs::bsc::client::BscWsClient;

/// Subscribe to v2 pair Sync events and emit PriceQuote updates for 1 unit of `token_in` to `token_out`.
pub async fn subscribe_v2_price<'a, P: Provider + Clone + 'static>(
    provider: P,
    ws: &'a BscWsClient,
    token_in: Address,
    token_out: Address,
) -> Result<(mpsc::Receiver<PriceQuote>, JoinHandle<()>)> {
    // Resolve pair via factory
    let fac = IPancakeV2FactoryView::new(PANCAKE_V2_FACTORY, provider.clone());
    let pair = fac.getPair(token_in, token_out).call().await?;
    if pair == Address::ZERO {
        anyhow::bail!("no v2 pair for token pair");
    }

    let topic_sync: B256 = keccak256("Sync(uint112,uint112)".as_bytes());
    let filter = Filter::new().address(pair).event_signature(topic_sync);
    let (mut rx_logs, ws_handle) = ws.subscribe_logs(filter).await?;

    let (tx, rx) = mpsc::channel::<PriceQuote>(1024);
    let prov_clone = provider.clone();
    let token_in_c = token_in;
    let token_out_c = token_out;

    let handle = tokio::spawn(async move {
        while let Some(_log) = rx_logs.recv().await {
            if let Ok(quote) = get_price_v2(prov_clone.clone(), token_in_c, token_out_c).await {
                let _ = tx.send(quote).await;
            }
        }
        ws_handle.abort();
    });

    Ok((rx, handle))
}

/// Subscribe to v3 pool Swap events and emit PriceQuote updates for 1 unit of `token_in` to `token_out`.
/// If `fee` is None, the pool fee tier is auto-detected once at startup.
pub async fn subscribe_v3_price<'a, P: Provider + Clone + 'static>(
    provider: P,
    ws: &'a BscWsClient,
    token_in: Address,
    token_out: Address,
    fee: Option<u32>,
) -> Result<(mpsc::Receiver<PriceQuote>, JoinHandle<()>)> {
    // Resolve pool via factory
    let fac = IPancakeV3FactoryView::new(PANCAKE_V3_FACTORY, provider.clone());
    let chosen_fee = if let Some(f) = fee {
        f
    } else {
        // Minimal auto-discovery: try common fee tiers, token order both ways
        let tiers: [u32; 5] = [100, 500, 800, 2500, 10000];
        let mut found: Option<u32> = None;
        for f in tiers {
            let p1 = fac
                .getPool(token_in, token_out, U24::from(f))
                .call()
                .await?;
            if p1 != Address::ZERO {
                found = Some(f);
                break;
            }
            let p2 = fac
                .getPool(token_out, token_in, U24::from(f))
                .call()
                .await?;
            if p2 != Address::ZERO {
                found = Some(f);
                break;
            }
        }
        found.ok_or_else(|| anyhow::anyhow!("no v3 pool found for token pair"))?
    };

    let pool = fac
        .getPool(token_in, token_out, U24::from(chosen_fee))
        .call()
        .await?;
    let pool_addr = if pool == Address::ZERO {
        // Try reversed order
        fac.getPool(token_out, token_in, U24::from(chosen_fee))
            .call()
            .await?
    } else {
        pool
    };
    if pool_addr == Address::ZERO {
        anyhow::bail!("no v3 pool for token pair");
    }

    let topic_swap: B256 =
        keccak256("Swap(address,address,int256,int256,uint160,uint128,int24)".as_bytes());
    let filter = Filter::new().address(pool_addr).event_signature(topic_swap);
    let (mut rx_logs, ws_handle) = ws.subscribe_logs(filter).await?;

    let (tx, rx) = mpsc::channel::<PriceQuote>(1024);
    let prov_clone = provider.clone();
    let token_in_c = token_in;
    let token_out_c = token_out;
    let fee_c = chosen_fee;

    let handle = tokio::spawn(async move {
        while let Some(_log) = rx_logs.recv().await {
            if let Ok(quote) =
                get_price_v3(prov_clone.clone(), token_in_c, token_out_c, Some(fee_c)).await
            {
                let _ = tx.send(quote).await;
            }
        }
        ws_handle.abort();
    });

    Ok((rx, handle))
}
