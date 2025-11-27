use anyhow::Result;

use alloy::primitives::{Address, B256, keccak256};
use alloy::providers::Provider;
use alloy::pubsub::Subscription;
use alloy::rpc::types::eth::{Filter, Log as RpcLog};

use crate::pancake::pancake_swap_v2::addresses::PANCAKE_V2_FACTORY;

#[derive(Clone, Debug)]
pub struct V2PairCreated {
    pub token0: Address,
    pub token1: Address,
    pub pair: Address,
}

pub fn v2_pair_created_topic() -> B256 {
    keccak256("PairCreated(address,address,address,uint256)".as_bytes())
}

pub fn v2_pair_created_filter() -> Filter {
    let topic0 = v2_pair_created_topic();
    let f = Filter::new();
    f.address(PANCAKE_V2_FACTORY).event_signature(topic0)
}

/// Subscribe to new Pancake v2 pairs created by the factory. Returns raw log stream.
pub async fn subscribe_v2_pair_created_logs<P: Provider + Clone>(provider: P) -> Result<Subscription<RpcLog>> {
    let filter = v2_pair_created_filter();
    Ok(provider.subscribe_logs(&filter).await?)
}

fn topic_to_address(topic: &B256) -> Address {
    let t = topic.as_slice();
    let mut bytes = [0u8; 20];
    bytes.copy_from_slice(&t[12..32]);
    Address::from(bytes)
}

/// Parse token0/token1 from topics.
pub fn try_parse_v2_pair_topics(log: &RpcLog) -> Option<(Address, Address)> {
    if log.topics().get(0).copied() != Some(v2_pair_created_topic()) {
        return None;
    }
    let topics = log.topics();
    if topics.len() < 3 { return None; }
    Some((topic_to_address(&topics[1]), topic_to_address(&topics[2])))
}

// Note: decoding pair address from data omitted here due to type differences in Log data representation.
// We resolve the pair via factory in `enrich_v2_pair_created` and skip entries if zero.

alloy::sol! {
    #[sol(rpc)]
    interface IPancakeV2FactoryView {
        function getPair(address tokenA, address tokenB) view returns (address pair);
    }
}

/// Resolve pair address via on-chain call.
pub async fn enrich_v2_pair_created<P: Provider + Clone>(provider: P, token0: Address, token1: Address) -> Result<V2PairCreated> {
    let fac = IPancakeV2FactoryView::new(PANCAKE_V2_FACTORY, provider);
    let pair = fac.getPair(token0, token1).call().await?;
    Ok(V2PairCreated { token0, token1, pair })
}


