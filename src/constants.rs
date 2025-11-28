use serde_json::Value;
use std::{collections::HashSet, sync::LazyLock};

#[macro_export]
macro_rules! env_lazy {
    ($( $vis:vis $name:ident : $ty:ty = ($key:literal, $default:expr); )* ) => {
        $(
            $vis static $name: ::std::sync::LazyLock<$ty> = ::std::sync::LazyLock::new(|| {
                $crate::libs::config::load_env();
                $crate::libs::config::Config::get_var_t::<$ty>($key, $default)
            });
        )*
    };
}

env_lazy! {
    pub MIN_TERMINAL_HEIGHT: u16 = ("MIN_TERMINAL_HEIGHT", 30);
    pub MAX_PAIRS: usize     = ("MAX_PAIRS", 120);
    pub BSC_CHAIN_ID: u64    = ("BSC_CHAIN_ID", 56);
}

pub const ALL_QUOTES: [&str; 6] = ["BNB", "CAKE", "USDT", "USD1", "ASTER", "WBNB"];

pub static AVOID_NAMES: LazyLock<HashSet<String>> = LazyLock::new(|| {
    let data = std::fs::read_to_string("names.json").unwrap_or_default();
    if let Ok(val) = serde_json::from_str::<Value>(&data) {
        if let Some(arr) = val.as_array() {
            return arr
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_ascii_lowercase())
                .collect();
        }
    }
    HashSet::new()
});

pub fn should_avoid_name<S: AsRef<str>>(name: S) -> bool {
    let n = name.as_ref().to_ascii_lowercase();
    AVOID_NAMES
        .iter()
        .any(|bad| !bad.is_empty() && n.contains(bad))
}
