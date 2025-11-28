use std::io::{self, Write};

use anyhow::Result;

use alloy::primitives::{Address, B256};
use alloy::providers::Provider;
use alloy::rpc::types::eth::TransactionReceipt;

use std::env;
use std::fs::{create_dir_all, OpenOptions};

alloy::sol! {
    #[sol(rpc)]
    interface IERC20Meta {
        function symbol() view returns (string);
        function name() view returns (string);
        function decimals() view returns (uint8);
    }
}

/// Read ERC20 `symbol()`; on failure returns a short hex of the address.
pub async fn addr_to_symbol<P: Provider + Clone>(provider: P, token: Address) -> Result<String> {
    let erc = IERC20Meta::new(token, provider);
    match erc.symbol().call().await {
        Ok(sym) => Ok(sym),
        Err(_) => {
            let s = token.as_slice();
            Ok(format!(
                "0x{}…{}",
                hex::encode(&s[0..3]),
                hex::encode(&s[17..20])
            ))
        }
    }
}

/// Read ERC20 `name()`; on failure returns the address as string.
pub async fn addr_to_name<P: Provider + Clone>(provider: P, token: Address) -> Result<String> {
    let erc = IERC20Meta::new(token, provider);
    match erc.name().call().await {
        Ok(n) => Ok(n),
        Err(_) => Ok(format!("{token:?}")),
    }
}

/// Fetch transaction receipt if available.
pub async fn tx_receipt<P: Provider + Clone>(
    provider: P,
    hash: B256,
) -> Result<Option<TransactionReceipt>> {
    let r = provider.get_transaction_receipt(hash).await?;
    Ok(r)
}

/// Simple known token search over baked-in Pancake addresses.
/// Matches by symbol (case-insensitive) or prefix of address (0x...).
pub fn search_known_tokens(query: &str) -> Vec<(String, Address)> {
    use pancakes::pancake::pancake_swap::addresses::*;
    let book: Vec<(String, Address)> = vec![
        ("WBNB".into(), WBNB),
        ("USDT".into(), USDT),
        ("BTCB".into(), BTCB),
    ];

    let q = query.to_lowercase();
    book.into_iter()
        .filter(|(sym, addr)| {
            let sym_l = sym.to_lowercase();
            if sym_l.contains(&q) {
                return true;
            }
            let addr_s = format!("{addr:?}");
            addr_s.to_lowercase().starts_with(&q)
        })
        .collect()
}

pub fn save_log_to_file(log: &str) {
    // skip if not enabled
    if !is_debug_logs_enabled() {
        return;
    }

    // Ensure logs directory exists at project root (current working directory)
    if let Err(e) = create_dir_all("logs") {
        eprintln!("save_log_to_file mkdir error: {e}");
        return;
    }
    let now = chrono::Utc::now().format("%H-%d-%m-%Y").to_string();
    let file_name = format!("logs/logs_{}.txt", now);
    let time_now = chrono::Utc::now().format("%H:%M:%S%.3f").to_string();
    let log_with_time = format!("[{}] {log}", time_now);

    if let Err(e) = append_line(&file_name, &log_with_time) {
        eprintln!("save_log_to_file error: {e}");
    }
}

fn is_debug_logs_enabled() -> bool {
    match env::var("DEBUG_LOGS") {
        Ok(val) => val.eq_ignore_ascii_case("true"),
        Err(_) => false,
    }
}

fn append_line(path: &str, line: &str) -> io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{line}")?;
    Ok(())
}

pub fn trim_chars(s: &str, max: usize) -> String {
    if s.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for ch in s.chars().take(max) {
        out.push(ch);
    }
    out
}
