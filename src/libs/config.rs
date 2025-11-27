use {
    dotenv::dotenv,
    serde::{Deserialize, Serialize},
    std::{fmt::Debug, str::FromStr},
};

pub fn load_env() {
    dotenv().ok();
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub bsc_wss: String,
    pub private_key: String,
    pub bsc_rpc: String,
}

impl Config {
    pub fn new() -> Self {
        Self {
            bsc_wss: std::env::var("BSC_WSS").expect("config.rs:BSC_WSS is not set"),
            private_key: std::env::var("PRIVATE_KEY").expect("config.rs: PRIVATE_KEY is not set"),
            bsc_rpc: std::env::var("BSC_RPC")
                .unwrap_or_else(|_| "https://bsc-dataseed.binance.org".to_string()),
        }
    }

    /// Parse env var to T; fall back to typed default.
    pub fn get_var_t<T>(key: &str, default: T) -> T
    where
        T: FromStr,
        <T as FromStr>::Err: Debug,
    {
        std::env::var(key)
            .ok()
            .and_then(|s| s.parse::<T>().ok())
            .unwrap_or(default)
    }
}
