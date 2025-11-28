#![allow(unused_imports)]

use alloy::primitives::utils::parse_units;
use alloy::primitives::U256;
use anyhow::Result;
use jimmyb::app::handler;

#[tokio::main]
async fn main() -> Result<()> {
    handler::init().await
}

pub fn format_bnb_hum(amount: String) -> U256 {
    match alloy::primitives::utils::parse_units(amount.as_str(), "ether") {
        Ok(parsed) => parsed.into(),
        Err(_) => U256::ZERO,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::U256;
    use std::str::FromStr;
    use url::Url;

    use anyhow::Result;

    use alloy::providers::ProviderBuilder;
    use alloy::providers::WalletProvider;
    use alloy::signers::local::PrivateKeySigner;
    use alloy::signers::Signer;
    use fourmeme::abi::IERC20Meta as FmErc20;
    use fourmeme::{self, addresses::TOKEN_MANAGER_HELPER_3};

    use alloy::primitives::keccak256;
    use alloy::primitives::Address;
    use alloy::rpc::types::eth::Filter;
    use jimmyb::libs::bsc::client::BscWsClient;
    use jimmyb::libs::config::load_env;
    use jimmyb::libs::config::Config;
    use jimmyb::libs::price::{subscribe_v2_price, subscribe_v3_price};
    use jimmyb::log;
    use jimmyb::routy::v2 as routy_v2;
    use jimmyb::routy::v3 as routy_v3;
    use jimmyb::routy::{sell_percent_to_wbnb_simple, swap_wbnb_to_token_simple};
    use pancakes::pancake::pancake_swap::addresses::PANCAKE_V3_FACTORY;
    use pancakes::pancake::pancake_swap::addresses::*;
    use pancakes::pancake::pancake_swap::router::format_token;
    use pancakes::pancake::pancake_swap::PancakeV3;
    use pancakes::pancake::pancake_swap_v2::addresses::PANCAKE_V2_FACTORY;
    use pancakes::pancake::pancake_swap_v2::router::format_token as format_token_v2;
    use pancakes::pancake::pancake_swap_v2::PancakeV2;
    use pancakes::plug::price::{get_price_v2, get_price_v3};
    use pancakes::plug::v2::{
        enrich_v2_pair_created, try_parse_v2_pair_topics, v2_pair_created_topic,
    };
    use pancakes::plug::{enrich_v3_pool_created, try_parse_v3_pool_topics};
    use pancakes::writing::cc;

    async fn fm_provider() -> Result<impl alloy::providers::Provider + Clone> {
        load_env();
        let rpc = std::env::var("BSC_RPC")?;
        let url = Url::parse(&rpc)?;
        let pk = std::env::var("PRIVATE_KEY")?;
        let signer = PrivateKeySigner::from_str(&pk)?.with_chain_id(Some(56));
        Ok(ProviderBuilder::new()
            .with_chain_id(56)
            .wallet(signer)
            .connect_http(url))
    }

    async fn fm_token_from_env() -> Result<alloy::primitives::Address> {
        use alloy::primitives::Address;
        let token_str = std::env::var("FOUR_TOKEN")?;
        let addr: Address = token_str.parse()?;
        Ok(addr)
    }

    async fn fm_token_decimals<P: alloy::providers::Provider + Clone>(
        provider: P,
        token: alloy::primitives::Address,
    ) -> u32 {
        FmErc20::new(token, provider)
            .decimals()
            .call()
            .await
            .unwrap_or(18) as u32
    }

    #[tokio::test]
    async fn test_fourmeme_try_buy_funds() -> Result<()> {
        let provider = fm_provider().await?;
        let token = fm_token_from_env().await?;
        let decimals = fm_token_decimals(provider.clone(), token).await;

        // Spend a tiny amount of BNB (0.001)
        let amount = U256::ZERO;
        let funds = parse_units("0.001", 18).unwrap().into();
        let quote = fourmeme::trade::try_buy(provider.clone(), token, amount, funds).await?;
        log!(
            cc::LIGHT_GREEN,
            "fourmeme try_buy funds: estimated_amount={}",
            fourmeme::price::format_units(quote.estimated_amount, decimals)
        );
        log!(
            cc::LIGHT_GREEN,
            "fourmeme try_buy funds: estimated_cost={}",
            fourmeme::price::format_units(quote.estimated_cost, 18)
        );
        log!(
            cc::LIGHT_GREEN,
            "fourmeme try_buy funds: estimated_fee={}",
            fourmeme::price::format_units(quote.estimated_fee, 18)
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_fourmeme_buy_real_amap() -> Result<()> {
        load_env();
        use alloy::signers::local::PrivateKeySigner;
        use alloy::signers::Signer;
        use jimmyb::log;
        use pancakes::writing::cc;

        let provider = fm_provider().await?;
        let token = fm_token_from_env().await?;

        // From signer
        let pk = std::env::var("PRIVATE_KEY")?;
        let from = PrivateKeySigner::from_str(&pk)?
            .with_chain_id(Some(56))
            .address();

        // Buy funds from env (BNB)
        let funds_str = std::env::var("FM_BNB_AMOUNT").unwrap_or_else(|_| "0.0001".to_string());
        let funds = format_bnb_hum(funds_str);

        let slippage_bps: u32 = std::env::var("FM_SLIPPAGE_BPS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(100);

        let router = jimmyb::router::FmRouter::new(provider.clone());
        let (est_amount, tx) = router
            .buy_with_bnb_amap(
                from,
                token,
                funds,
                slippage_bps,
                None,
                U256::ZERO.try_into().unwrap(),
            )
            .await?;

        let decs = fm_token_decimals(provider.clone(), token).await;
        let est_amt_str = fourmeme::price::format_units(est_amount, decs);
        log!(
            cc::LIGHT_GREEN,
            "fourmeme buy (AMAP): est_amount={}",
            est_amt_str
        );
        log!(cc::LIGHT_GREEN, "fourmeme buy (AMAP): tx={:?}", tx);
        Ok(())
    }

    #[tokio::test]
    async fn test_fourmeme_sell_real_pct() -> Result<()> {
        load_env();
        use alloy::signers::local::PrivateKeySigner;
        use alloy::signers::Signer;
        use jimmyb::log;
        use pancakes::writing::cc;

        let provider = fm_provider().await?;
        let token = fm_token_from_env().await?;

        // From signer
        let pk = std::env::var("PRIVATE_KEY")?;
        let from = PrivateKeySigner::from_str(&pk)?
            .with_chain_id(Some(56))
            .address();

        // Sell percent from env (0..=100), default 1%
        let pct_points: u32 = std::env::var("FM_SELL_PCT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(100);

        let router = jimmyb::router::FmRouter::new(provider.clone());
        let (est_funds, tx) = router
            .sell_percent_pct(from, token, pct_points, U256::ZERO.try_into().unwrap())
            .await?;

        let est_funds_str = fourmeme::price::format_units(est_funds, 18);
        log!(
            cc::LIGHT_GREEN,
            "fourmeme sell: est_funds={} BNB",
            est_funds_str
        );
        log!(cc::LIGHT_GREEN, "fourmeme sell: tx={:?}", tx);
        Ok(())
    }

    #[tokio::test]
    async fn test_fourmeme_try_sell() -> Result<()> {
        let provider = fm_provider().await?;
        let token = fm_token_from_env().await?;
        // Sell 0.1% of wallet balance
        let pk = std::env::var("PRIVATE_KEY")?;
        let from = PrivateKeySigner::from_str(&pk)?
            .with_chain_id(Some(56))
            .address();
        let quote = fourmeme::trade::try_sell_pct(provider.clone(), token, from, 10).await?; // 10 bips = 0.1%
        log!(
            cc::LIGHT_GREEN,
            "fourmeme try_sell: funds={}",
            fourmeme::price::format_units(quote.funds, 18)
        );
        log!(
            cc::LIGHT_GREEN,
            "fourmeme try_sell: fee={}",
            fourmeme::price::format_units(quote.fee, 18)
        );
        Ok(())
    }
    #[tokio::test]
    async fn test_sell_btcb_10pct_to_wbnb_via_routy() -> Result<()> {
        load_env();

        let rpc = std::env::var("BSC_RPC")?;
        let url = Url::parse(&rpc)?;
        let pk = std::env::var("PRIVATE_KEY")?;
        let signer = PrivateKeySigner::from_str(&pk)?.with_chain_id(Some(56));

        let provider = ProviderBuilder::new()
            .with_chain_id(56)
            .wallet(signer)
            .connect_http(url);

        let from = provider.wallet().default_signer().address();
        let pancake = PancakeV3::new(provider.clone());

        // Use a small percentage to avoid spending too much
        let (quoted, tx) = sell_percent_to_wbnb_simple(
            &pancake,
            from,
            "0x7130d2A12B9BCbFAe4f2634d864A1Ee1Ce3EaD9c", // BTCB
            1_000,                                        // 10%
        )
        .await?;

        log!(cc::LIGHT_GREEN, "quoted_out: {}", format_token(quoted, 18));
        log!(cc::LIGHT_GREEN, "tx: {}", tx);
        Ok(())
    }

    #[tokio::test]
    async fn test_swap_wbnb_to_token_via_routy() -> Result<()> {
        load_env();

        let rpc = std::env::var("BSC_RPC")?;
        let url = Url::parse(&rpc)?;
        let pk = std::env::var("PRIVATE_KEY")?;
        let signer = PrivateKeySigner::from_str(&pk)?.with_chain_id(Some(56));
        let bnb_in_amount_str =
            std::env::var("BNB_AMOUNT").unwrap_or_else(|_| "0.0003".to_string());
        log!(
            cc::YELLOW,
            "BNB in amount (WBNB spend): {}",
            bnb_in_amount_str
        );

        let provider = ProviderBuilder::new()
            .with_chain_id(56)
            .wallet(signer)
            .connect_http(url);

        let from = provider.wallet().default_signer().address();
        let pancake = PancakeV3::new(provider.clone());

        let (quoted, tx) = swap_wbnb_to_token_simple(
            &pancake,
            from,
            BTCB.to_string().as_str(), // BTCB
            format_bnb_hum(bnb_in_amount_str),
        )
        .await?;

        log!(cc::LIGHT_GREEN, "quoted_out: {}", format_token(quoted, 18));
        log!(cc::LIGHT_GREEN, "tx: {}", tx);
        Ok(())
    }

    #[tokio::test]
    async fn test_static_prices_v2_v3() -> Result<()> {
        load_env();

        let rpc = std::env::var("BSC_RPC")?;
        let url = Url::parse(&rpc)?;
        let pk = std::env::var("PRIVATE_KEY")?;
        let signer = PrivateKeySigner::from_str(&pk)?.with_chain_id(Some(56));

        let provider = ProviderBuilder::new()
            .with_chain_id(56)
            .wallet(signer)
            .connect_http(url);

        let v2_quote = get_price_v2(provider.clone(), WBNB, USDT).await?;
        log!(
            cc::LIGHT_GREEN,
            "v2: 1 WBNB -> {} USDT",
            format_token_v2(v2_quote.amount_out_base_units, v2_quote.decimals_out)
        );
        log!(
            cc::LIGHT_GREEN,
            "v2: raw out: {}",
            v2_quote.amount_out_base_units
        );
        let v3_quote = get_price_v3(provider.clone(), WBNB, USDT, Some(500)).await?;
        log!(
            cc::LIGHT_GREEN,
            "v3: 1 WBNB -> {} USDT",
            format_token(v3_quote.amount_out_base_units, v3_quote.decimals_out)
        );
        log!(
            cc::LIGHT_GREEN,
            "v3: raw out: {}",
            v3_quote.amount_out_base_units
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_ws_price_subscriptions_brief() -> Result<()> {
        load_env();

        let rpc = std::env::var("BSC_RPC")?;
        let wss = std::env::var("BSC_WSS")
            .unwrap_or_else(|_| "wss://bsc-ws-node.nariox.org:443".to_string());
        let url = Url::parse(&rpc)?;
        let pk = std::env::var("PRIVATE_KEY")?;
        let signer = PrivateKeySigner::from_str(&pk)?.with_chain_id(Some(56));

        let provider = ProviderBuilder::new()
            .with_chain_id(56)
            .wallet(signer)
            .connect_http(url);

        let ws = BscWsClient::new(wss, std::env::var("PRIVATE_KEY")?).await?;

        let (mut v2_rx, _) = subscribe_v2_price(
            provider.clone(),
            &ws,
            "0x6B64e048b90e7F5628053eBC962a6321db3f4444"
                .parse()
                .unwrap(),
            WBNB,
        )
        .await?;

        loop {
            let _ = match v2_rx.recv().await {
                Some(q) => {
                    log!(
                        cc::LIGHT_GREEN,
                        "v2 price update for token: {}",
                        format_token(q.amount_in_base_units, q.decimals_in)
                    );
                    log!(
                        cc::LIGHT_GREEN,
                        "v2 price update: raw out: {}",
                        q.amount_out_base_units
                    );
                }
                None => {
                    log!(cc::LIGHT_RED, "no price update received");
                }
            };
        }
    }

    #[tokio::test]
    async fn test_sell_btcb_via_v2() -> Result<()> {
        load_env();

        let rpc = std::env::var("BSC_RPC")?;
        let url = Url::parse(&rpc)?;
        let pk = std::env::var("PRIVATE_KEY")?;
        let signer = PrivateKeySigner::from_str(&pk)?.with_chain_id(Some(56));

        let provider = ProviderBuilder::new()
            .with_chain_id(56)
            .wallet(signer)
            .connect_http(url);

        let from = provider.wallet().default_signer().address();
        let pancake = PancakeV2::new(provider.clone());

        let (quoted, tx) = routy_v2::sell_pct_to_wbnb(
            &pancake,
            from,
            "0x7130d2A12B9BCbFAe4f2634d864A1Ee1Ce3EaD9c", // BTCB
            1_000,                                        // 10%
            None,
        )
        .await?;

        log!(
            cc::LIGHT_GREEN,
            "quoted_out (v2): {}",
            format_token_v2(quoted, 18)
        );
        log!(cc::LIGHT_GREEN, "tx (v2): {}", tx);
        Ok(())
    }

    #[tokio::test]
    async fn test_buy_btcb_via_v2() -> Result<()> {
        load_env();

        let rpc = std::env::var("BSC_RPC")?;
        let url = Url::parse(&rpc)?;
        let pk = std::env::var("PRIVATE_KEY")?;
        let signer = PrivateKeySigner::from_str(&pk)?.with_chain_id(Some(56));
        let bnb_in_amount_str =
            std::env::var("BNB_AMOUNT").unwrap_or_else(|_| "0.0003".to_string());
        log!(
            cc::YELLOW,
            "BNB in amount (WBNB spend, v2): {}",
            bnb_in_amount_str
        );

        let provider = ProviderBuilder::new()
            .with_chain_id(56)
            .wallet(signer)
            .connect_http(url);

        let from = provider.wallet().default_signer().address();
        let pancake = PancakeV2::new(provider.clone());

        let (quoted, tx) = routy_v2::swap_wbnb_to(
            &pancake,
            from,
            BTCB.to_string().as_str(), // BTCB
            format_bnb_hum(bnb_in_amount_str),
            None,
        )
        .await?;

        log!(
            cc::LIGHT_GREEN,
            "quoted_out (v2): {}",
            format_token_v2(quoted, 18)
        );
        log!(cc::LIGHT_GREEN, "tx (v2): {}", tx);
        Ok(())
    }

    #[tokio::test]
    async fn test_buy_usdt_via_v3() -> Result<()> {
        load_env();

        let rpc = std::env::var("BSC_RPC")?;
        let url = Url::parse(&rpc)?;
        let pk = std::env::var("PRIVATE_KEY")?;
        let signer = PrivateKeySigner::from_str(&pk)?.with_chain_id(Some(56));
        let bnb_in_amount_str =
            std::env::var("BNB_AMOUNT").unwrap_or_else(|_| "0.0003".to_string());
        log!(
            cc::YELLOW,
            "BNB in amount (WBNB spend, v3): {}",
            bnb_in_amount_str
        );

        let provider = ProviderBuilder::new()
            .with_chain_id(56)
            .wallet(signer)
            .connect_http(url);

        let from = provider.wallet().default_signer().address();
        let pancake = PancakeV3::new(provider.clone());

        let (quoted, tx) = routy_v3::swap_token_to(
            &pancake,
            from,
            WBNB.to_string().as_str(),
            USDT.to_string().as_str(),
            format_bnb_hum(bnb_in_amount_str),
            None,
        )
        .await?;

        log!(
            cc::LIGHT_GREEN,
            "quoted_out (v3, USDT): {}",
            format_token(quoted, 18)
        );
        log!(cc::LIGHT_GREEN, "tx (v3, USDT): {}", tx);
        Ok(())
    }

    #[tokio::test]
    async fn test_buy_usdt_via_v2() -> Result<()> {
        load_env();

        let rpc = std::env::var("BSC_RPC")?;
        let url = Url::parse(&rpc)?;
        let pk = std::env::var("PRIVATE_KEY")?;
        let signer = PrivateKeySigner::from_str(&pk)?.with_chain_id(Some(56));
        let bnb_in_amount_str =
            std::env::var("BNB_AMOUNT").unwrap_or_else(|_| "0.0003".to_string());
        log!(
            cc::YELLOW,
            "BNB in amount (WBNB spend, v2): {}",
            bnb_in_amount_str
        );

        let provider = ProviderBuilder::new()
            .with_chain_id(56)
            .wallet(signer)
            .connect_http(url);

        let from = provider.wallet().default_signer().address();
        let pancake = PancakeV2::new(provider.clone());

        let (quoted, tx) = routy_v2::swap_token_to(
            &pancake,
            from,
            WBNB.to_string().as_str(),
            USDT.to_string().as_str(),
            format_bnb_hum(bnb_in_amount_str),
            None,
        )
        .await?;

        log!(
            cc::LIGHT_GREEN,
            "quoted_out (v2, USDT): {}",
            format_token_v2(quoted, 18)
        );
        log!(cc::LIGHT_GREEN, "tx (v2, USDT): {}", tx);
        Ok(())
    }

    #[tokio::test]
    async fn test_swap_btcb_to_usdt_via_v3() -> Result<()> {
        load_env();

        let rpc = std::env::var("BSC_RPC")?;
        let url = Url::parse(&rpc)?;
        let pk = std::env::var("PRIVATE_KEY")?;
        let signer = PrivateKeySigner::from_str(&pk)?.with_chain_id(Some(56));
        let btcb_in_amount_str =
            std::env::var("BTCB_AMOUNT").unwrap_or_else(|_| "0.00001".to_string());
        log!(
            cc::YELLOW,
            "BTCB in amount (token spend, v3): {}",
            btcb_in_amount_str
        );

        let provider = ProviderBuilder::new()
            .with_chain_id(56)
            .wallet(signer)
            .connect_http(url);

        let from = provider.wallet().default_signer().address();
        let pancake = PancakeV3::new(provider.clone());

        let (quoted, tx) = routy_v3::swap_token_to(
            &pancake,
            from,
            BTCB.to_string().as_str(),
            USDT.to_string().as_str(),
            format_bnb_hum(btcb_in_amount_str),
            None,
        )
        .await?;

        log!(
            cc::LIGHT_GREEN,
            "quoted_out (v3, BTCB->USDT): {}",
            format_token(quoted, 18)
        );
        log!(cc::LIGHT_GREEN, "tx (v3, BTCB->USDT): {}", tx);
        Ok(())
    }

    #[tokio::test]
    async fn test_swap_btcb_to_usdt_via_v2() -> Result<()> {
        load_env();

        let rpc = std::env::var("BSC_RPC")?;
        let url = Url::parse(&rpc)?;
        let pk = std::env::var("PRIVATE_KEY")?;
        let signer = PrivateKeySigner::from_str(&pk)?.with_chain_id(Some(56));
        let btcb_in_amount_str =
            std::env::var("BTCB_AMOUNT").unwrap_or_else(|_| "0.00001".to_string());
        log!(
            cc::YELLOW,
            "BTCB in amount (token spend, v2): {}",
            btcb_in_amount_str
        );

        let provider = ProviderBuilder::new()
            .with_chain_id(56)
            .wallet(signer)
            .connect_http(url);

        let from = provider.wallet().default_signer().address();
        let pancake = PancakeV2::new(provider.clone());

        let (quoted, tx) = routy_v2::swap_token_to(
            &pancake,
            from,
            BTCB.to_string().as_str(),
            USDT.to_string().as_str(),
            format_bnb_hum(btcb_in_amount_str),
            None,
        )
        .await?;

        log!(
            cc::LIGHT_GREEN,
            "quoted_out (v2, BTCB->USDT): {}",
            format_token_v2(quoted, 18)
        );
        log!(cc::LIGHT_GREEN, "tx (v2, BTCB->USDT): {}", tx);
        Ok(())
    }

    #[tokio::test]
    async fn test_ws_plug_v3_pool_created_subscribe() -> Result<()> {
        load_env();

        let cfg = Config::new();
        let ws = BscWsClient::new(cfg.bsc_wss.clone(), cfg.private_key.clone()).await?;

        let topic0 = keccak256("PoolCreated(address,address,uint24,int24,address)".as_bytes());
        let filter = Filter::new()
            .address(PANCAKE_V3_FACTORY)
            .event_signature(topic0);

        let (mut rx, handle) = ws.subscribe_logs(filter).await?;

        // Try to receive one PoolCreated log quickly; do not fail if none arrives.
        match rx.recv().await {
            Some(log) => {
                if let Some((t0, t1)) = try_parse_v3_pool_topics(&log) {
                    let rpc = std::env::var("BSC_RPC")?;
                    let url = Url::parse(&rpc)?;
                    let pk = std::env::var("PRIVATE_KEY")?;
                    let signer = PrivateKeySigner::from_str(&pk)?.with_chain_id(Some(56));
                    let provider = ProviderBuilder::new()
                        .with_chain_id(56)
                        .wallet(signer)
                        .connect_http(url);

                    let info = enrich_v3_pool_created(provider.clone(), t0, t1).await?;
                    log!(cc::LIGHT_GREEN, "v3 PoolCreated: pool={}", info.pool);
                    log!(cc::LIGHT_GREEN, "v3 PoolCreated: token0={}", info.token0);
                    log!(cc::LIGHT_GREEN, "v3 PoolCreated: token1={}", info.token1);
                    log!(cc::LIGHT_GREEN, "v3 PoolCreated: fee={}", info.fee);
                    log!(
                        cc::LIGHT_GREEN,
                        "v3 PoolCreated: tick_spacing={}",
                        info.tick_spacing
                    );
                } else {
                    log!(
                        cc::LIGHT_YELLOW,
                        "received log but topic didn't match PoolCreated"
                    );
                }
            }
            None => {
                log!(cc::LIGHT_YELLOW, "no log received");
            }
        };

        handle.abort();
        Ok(())
    }

    #[tokio::test]
    async fn test_ws_plug_v2_pool_created_subscribe() -> Result<()> {
        load_env();

        let cfg = Config::new();
        let ws = BscWsClient::new(cfg.bsc_wss.clone(), cfg.private_key.clone()).await?;

        let topic0 = v2_pair_created_topic();
        let filter = Filter::new()
            .address(PANCAKE_V2_FACTORY)
            .event_signature(topic0);

        let (mut rx, handle) = ws.subscribe_logs(filter).await?;

        // Try to receive one PoolCreated log quickly; do not fail if none arrives.
        match rx.recv().await {
            Some(log) => {
                if let Some((t0, t1)) = try_parse_v2_pair_topics(&log) {
                    let rpc = std::env::var("BSC_RPC")?;
                    let url = Url::parse(&rpc)?;
                    let pk = std::env::var("PRIVATE_KEY")?;
                    let signer = PrivateKeySigner::from_str(&pk)?.with_chain_id(Some(56));
                    let provider = ProviderBuilder::new()
                        .with_chain_id(56)
                        .wallet(signer)
                        .connect_http(url);

                    let info = enrich_v2_pair_created(provider.clone(), t0, t1).await?;
                    log!(cc::LIGHT_GREEN, "v2 PairCreated: pair={}", info.pair);
                    log!(cc::LIGHT_GREEN, "v2 PairCreated: token0={}", info.token0);
                    log!(cc::LIGHT_GREEN, "v2 PairCreated: token1={}", info.token1);
                } else {
                    log!(
                        cc::LIGHT_YELLOW,
                        "received log but topic didn't match PairCreated"
                    );
                }
            }
            None => {
                log!(cc::LIGHT_YELLOW, "no log received");
            }
        };

        handle.abort();
        Ok(())
    }
}
