use bytes::{BufMut, BytesMut};
use alloy::primitives::{Bytes, U256, Address};

pub fn encode_v3_path(tokens: &[Address], fees: &[u32]) -> Bytes {
    assert!(!tokens.is_empty(), "path needs at least one token");
    assert_eq!(tokens.len(), fees.len() + 1, "tokens = fees + 1");

    let mut out = BytesMut::with_capacity(tokens.len() * 20 + fees.len() * 3);
    for i in 0..fees.len() {
        out.put_slice(tokens[i].as_slice());
        let f = fees[i] as u32;
        out.put_u8(((f >> 16) & 0xff) as u8);
        out.put_u8(((f >> 8) & 0xff) as u8);
        out.put_u8((f & 0xff) as u8);
    }
    out.put_slice(tokens.last().unwrap().as_slice());
    Bytes::from(out.freeze())
}

pub fn apply_slippage_bps(quoted: U256, slippage_bps: u32) -> U256 {
    let bps = U256::from(10_000u64 - slippage_bps as u64);
    quoted * bps / U256::from(10_000u64)
}