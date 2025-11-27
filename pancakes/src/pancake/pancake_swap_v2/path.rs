use alloy::primitives::{Address, U256};

pub fn apply_slippage_bps(quoted: U256, slippage_bps: u32) -> U256 {
    let bps = U256::from(10_000u64 - slippage_bps as u64);
    quoted * bps / U256::from(10_000u64)
}

pub fn path2(token_in: Address, token_out: Address) -> Vec<Address> {
    vec![token_in, token_out]
}

pub fn path3(token_in: Address, mid: Address, token_out: Address) -> Vec<Address> {
    vec![token_in, mid, token_out]
}


