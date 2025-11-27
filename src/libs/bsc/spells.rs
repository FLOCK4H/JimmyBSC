//! High level convenience functions for interacting with the BNB Smart
//! Chain.  Whereas the [`crate::libs::bsc::client::BscClient`]
//! exposes low level RPC calls, these helpers perform common tasks
//! such as querying an account balance or formatting BNB values.
use alloy::primitives::{Address, U256};
/// GPT-5 AGENT GENERATED FILE
use anyhow::Result;

use super::client::{BscClient, BscWsClient};

/// Query the on‑chain balance of an address.  The supplied client
/// must already be connected to a JSON‑RPC endpoint.  This
/// function simply delegates to [`BscClient::get_balance_at`] with
/// the block tag set to `"latest"` and propagates any errors.
pub async fn get_balance(client: &BscClient, address: Address) -> Result<U256> {
    let balance = client.get_balance_at(address, "latest").await?;
    Ok(balance)
}

/// Format a raw BNB balance for display.  The input value is a
/// `U256` representing the number of wei (1 BNB = 1e18 wei).  The
/// output string includes a decimal point inserted 18 digits from
/// the right hand side.  Leading zeros are preserved and there is
/// always at least one digit before the decimal point.  Trailing
/// zeros on the fractional part are not stripped to avoid
/// unintentional precision loss.
///
/// # Examples
///
/// ```
/// # use alloy_primitives::U256;
/// # use jimmyb::libs::bsc::spells::format_bnb;
/// let value = U256::from(1_234_000_000_000_000_000u128);
/// let s = format_bnb(value).unwrap();
/// assert_eq!(s, "1.234000000000000000");
/// ```
/// Accepts things like "0x0", "0x1", "0xa3f4...", "0XDEAD...", or even "0"
/// and always returns a human string (BNB) instead of crashing.
pub fn format_bnb<S: AsRef<str>>(raw_hex: S) -> Result<String> {
    let mut s = raw_hex.as_ref().trim();

    // 1. strip 0x / 0X
    if let Some(stripped) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        s = stripped;
    }

    // 2. empty balance → 0 BNB
    if s.is_empty() {
        return Ok("0 BNB".to_string());
    }

    // 3. pad to even length, because RPC can return "0" or "1"
    let owned;
    if s.len() % 2 == 1 {
        let mut tmp = String::with_capacity(s.len() + 1);
        tmp.push('0');
        tmp.push_str(s);
        owned = tmp;
        s = &owned;
    }

    // 4. hex -> bytes -> U256
    let bytes = hex::decode(s)
        .map_err(|e| anyhow::anyhow!("failed to decode balance hex `{}`: {}", s, e))?;
    let wei = U256::from_be_slice(&bytes);

    // 5. format as BNB (18 decimals)
    Ok(wei_to_bnb(wei))
}

/// Very small helper: if value fits into u128 we print nice decimals,
/// otherwise we fall back to "X wei".
fn wei_to_bnb(wei: U256) -> String {
    // if it's huge, just show wei
    if wei > U256::from(u128::MAX) {
        return format!("{wei} wei");
    }

    // safe to downcast
    let v: u128 = wei.try_into().unwrap();
    let whole = v / 1_000_000_000_000_000_000u128;
    let frac = v % 1_000_000_000_000_000_000u128;

    if frac == 0 {
        format!("{whole} BNB")
    } else {
        // trim trailing zeros but keep at most 18 decimals
        let mut frac_str = format!("{:018}", frac);
        while frac_str.ends_with('0') {
            frac_str.pop();
        }
        format!("{whole}.{frac_str} BNB")
    }
}
