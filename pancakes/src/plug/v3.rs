use anyhow::Result;

use alloy::primitives::{Address, B256, keccak256,};
use alloy::providers::Provider;
use alloy::pubsub::Subscription;
use alloy::rpc::types::eth::{Filter, Log};

use crate::pancake::pancake_swap::addresses::PANCAKE_V3_FACTORY;

#[derive(Clone, Debug)]
pub struct V3PoolCreated {
    pub token0: Address,
    pub token1: Address,
    pub pool: Address,
    pub fee: u32,
    pub tick_spacing: i32,
}

pub fn v3_pool_created_topic() -> B256 {
    keccak256("PoolCreated(address,address,uint24,int24,address)".as_bytes())
}

pub fn v3_pool_created_filter() -> Filter {
    let topic0 = v3_pool_created_topic();
    let f = Filter::new();
    f.address(PANCAKE_V3_FACTORY).event_signature(topic0)
}

/// Subscribe to new Pancake v3 pools created by the factory. Returns raw log stream.
pub async fn subscribe_v3_pool_created_logs<P: Provider + Clone>(provider: P) -> Result<Subscription<Log>> {
    let filter = v3_pool_created_filter();
    Ok(provider.subscribe_logs(&filter).await?)
}

pub fn topic_to_address(topic: &B256) -> Address {
    let t = topic.as_slice();
    let mut bytes = [0u8; 20];
    bytes.copy_from_slice(&t[12..32]);
    Address::from(bytes)
}

/// Parse token0/token1 from topics.
pub fn try_parse_v3_pool_topics(log: &Log) -> Option<(Address, Address)> {
    if log.topics().get(0).copied() != Some(v3_pool_created_topic()) {
        return None;
    }
    let topics = log.topics();
    if topics.len() < 3 { return None; }
    Some((topic_to_address(&topics[1]), topic_to_address(&topics[2])))
}

alloy::sol! {
    #[sol(rpc)]
    interface IPancakeV3Factory {
        function getPool(address tokenA, address tokenB, uint24 fee) view returns (address pool);
    }
    #[sol(rpc)]
    interface IPancakeV3PoolView {
        function tickSpacing() view returns (int24);
        function fee() view returns (uint24);
    }
}

const V3_FEE_TIERS: [u32; 5] = [100, 500, 800, 2500, 10000];

/// Resolve full pool info (pool address, fee, tickSpacing) via on-chain calls.
pub async fn enrich_v3_pool_created<P: Provider + Clone>(provider: P, token0: Address, token1: Address) -> Result<V3PoolCreated> {
    let factory = IPancakeV3Factory::new(PANCAKE_V3_FACTORY, provider.clone());
    let mut found_pool = Address::ZERO;
    let mut found_fee: u32 = 0;
    for fee in V3_FEE_TIERS {
        let pool_addr = factory.getPool(token0, token1, fee.try_into()?).call().await?;
        if pool_addr != Address::ZERO {
            found_pool = pool_addr;
            found_fee = fee;
            break;
        }
    }
    if found_pool == Address::ZERO {
        return Err(anyhow::anyhow!("no pool found for token pair"));
    }
    let pool = IPancakeV3PoolView::new(found_pool, provider.clone());
    let ts: i32 = pool.tickSpacing().call().await?.try_into()?;
    Ok(V3PoolCreated {
        token0,
        token1,
        pool: found_pool,
        fee: found_fee,
        tick_spacing: ts,
    })
}


