use std::sync::LazyLock;

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
    pub MIN_TERMINAL_HEIGHT: u16 = ("MIN_TERMINAL_HEIGHT", 25);
    pub MAX_PAIRS: usize     = ("MAX_PAIRS", 120);
    pub BSC_CHAIN_ID: u64    = ("BSC_CHAIN_ID", 56);
}

pub const ALL_QUOTES: [&str; 6] = ["BNB", "CAKE", "USDT", "USD1", "ASTER", "WBNB"];
