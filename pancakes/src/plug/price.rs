use anyhow::Result;

use alloy::primitives::{Address, U256};
use alloy::primitives::aliases::{U160, U24};
use alloy::providers::Provider;

use crate::pancake::pancake_swap_v2::addresses::PANCAKE_V2_ROUTER;
use crate::pancake::pancake_swap::addresses::{PANCAKE_V3_QUOTER_V2, PANCAKE_V3_FACTORY};

/// A normalized price quote using 1 whole unit of `token_in` in human scale.
#[derive(Clone, Debug)]
pub struct PriceQuote {
    pub token_in: Address,
    pub token_out: Address,
    /// Raw amount in base units used (10^decimals_in)
    pub amount_in_base_units: U256,
    /// Raw amount out returned by the on-chain quoter/call
    pub amount_out_base_units: U256,
    pub decimals_in: u32,
    pub decimals_out: u32,
}

alloy::sol! {
    #[sol(rpc)]
    interface IERC20View {
        function decimals() view returns (uint8);
        function balanceOf(address owner) view returns (uint256);
    }

    // v2
    #[sol(rpc)]
    interface IPancakeRouter02View {
        function getAmountsOut(uint amountIn, address[] calldata path) external view returns (uint[] memory amounts);
    }

    // v3
    #[sol(rpc)]
    interface IQuoterV2View {
        struct QuoteExactInputSingleParams {
            address tokenIn;
            address tokenOut;
            uint256 amountIn;
            uint24  fee;
            uint160 sqrtPriceLimitX96;
        }
        function quoteExactInputSingle(QuoteExactInputSingleParams calldata params)
            returns (uint256 amountOut, uint160, uint32, uint256);
    }

    #[sol(rpc)]
    interface IPancakeV3FactoryView {
        function getPool(address tokenA, address tokenB, uint24 fee) view returns (address pool);
    }
}

const V3_FEE_TIERS: [u32; 5] = [100, 500, 800, 2500, 10000];

async fn token_decimals<P: Provider + Clone>(provider: P, token: Address) -> Result<u32> {
    let erc20 = IERC20View::new(token, provider);
    Ok(erc20.decimals().call().await? as u32)
}

fn one_unit(decimals: u32) -> U256 {
    U256::from(10u64).pow(U256::from(decimals))
}

/// Static price on v2 via router.getAmountsOut for 1 whole token_in.
pub async fn get_price_v2<P: Provider + Clone>(provider: P, token_in: Address, token_out: Address) -> Result<PriceQuote> {
    let dec_in = token_decimals(provider.clone(), token_in).await?;
    let dec_out = token_decimals(provider.clone(), token_out).await?;
    let amount_in = one_unit(dec_in);
    let router = IPancakeRouter02View::new(PANCAKE_V2_ROUTER, provider.clone());
    let path = vec![token_in, token_out];
    let amounts: Vec<U256> = router.getAmountsOut(amount_in, path).call().await?;
    let amount_out = amounts.last().cloned().unwrap_or(U256::ZERO);
    Ok(PriceQuote {
        token_in,
        token_out,
        amount_in_base_units: amount_in,
        amount_out_base_units: amount_out,
        decimals_in: dec_in,
        decimals_out: dec_out,
    })
}

/// Static price on v3 via QuoterV2 for 1 whole token_in. If `fee` is None, auto-detect.
pub async fn get_price_v3<P: Provider + Clone>(provider: P, token_in: Address, token_out: Address, fee: Option<u32>) -> Result<PriceQuote> {
    let dec_in = token_decimals(provider.clone(), token_in).await?;
    let dec_out = token_decimals(provider.clone(), token_out).await?;
    let amount_in = one_unit(dec_in);

    let chosen_fee = match fee {
        Some(f) => f,
        None => {
            let factory = IPancakeV3FactoryView::new(PANCAKE_V3_FACTORY, provider.clone());
            // Try both token orders to be safe
            let mut found: Option<u32> = None;
            for f in V3_FEE_TIERS {
                let p1 = factory.getPool(token_in, token_out, U24::from(f)).call().await?;
                if p1 != Address::ZERO { found = Some(f); break; }
                let p2 = factory.getPool(token_out, token_in, U24::from(f)).call().await?;
                if p2 != Address::ZERO { found = Some(f); break; }
            }
            found.ok_or_else(|| anyhow::anyhow!("no v3 pool found for token pair"))?
        }
    };

    let quoter = IQuoterV2View::new(PANCAKE_V3_QUOTER_V2, provider.clone());
    let params = IQuoterV2View::QuoteExactInputSingleParams {
        tokenIn: token_in,
        tokenOut: token_out,
        amountIn: amount_in,
        fee: U24::from(chosen_fee),
        sqrtPriceLimitX96: U160::ZERO,
    };
    let ret = quoter.quoteExactInputSingle(params).call().await?;
    let amount_out = ret.amountOut;

    Ok(PriceQuote {
        token_in,
        token_out,
        amount_in_base_units: amount_in,
        amount_out_base_units: amount_out,
        decimals_in: dec_in,
        decimals_out: dec_out,
    })
}

// ===== Liquidity helpers =====
alloy::sol! {
    #[sol(rpc)]
    interface IPancakePairViewLiq {
        function token0() view returns (address);
        function token1() view returns (address);
        function getReserves() view returns (uint112 reserve0, uint112 reserve1, uint32 blockTimestampLast);
    }
}

/// Return (quote_reserve_in_base_units, quote_decimals) for a v2 pair.
pub async fn get_liquidity_v2<P: Provider + Clone>(provider: P, pair: Address, quote: Address) -> Result<(U256, u32)> {
    let pairc = IPancakePairViewLiq::new(pair, provider.clone());
    let (t0, t1) = (pairc.token0().call().await?, pairc.token1().call().await?);
    let reserves = pairc.getReserves().call().await?;
    let (r0, r1) = (U256::from(reserves.reserve0), U256::from(reserves.reserve1));
    let dec_q = token_decimals(provider.clone(), quote).await?;
    let rq = if quote == t0 { r0 } else if quote == t1 { r1 } else { U256::ZERO };
    Ok((rq, dec_q))
}

/// Return (quote_reserve_in_base_units, quote_decimals) for a v3 pool by reading ERC-20 balanceOf(pool).
pub async fn get_liquidity_v3<P: Provider + Clone>(provider: P, pool: Address, quote: Address) -> Result<(U256, u32)> {
    let erc20 = IERC20View::new(quote, provider.clone());
    let rq = erc20.balanceOf(pool).call().await?;
    let dec_q = erc20.decimals().call().await? as u32;
    Ok((U256::from(rq), dec_q))
}


