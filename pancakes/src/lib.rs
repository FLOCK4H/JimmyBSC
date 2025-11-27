pub mod writing {
        pub mod cc {
        pub const RED: &str      = "\x1b[31m";
        pub const GREEN: &str    = "\x1b[32m";
        pub const YELLOW: &str   = "\x1b[33m";
        pub const BLUE: &str        = "\x1b[34m";
        pub const MAGENTA: &str     = "\x1b[35m";
        pub const CYAN: &str        = "\x1b[36m";
        pub const WHITE: &str    = "\x1b[37m";
        pub const BOLD: &str     = "\x1b[1m";
        pub const RESET: &str    = "\x1b[0m";
        pub const BLINK: &str    = "\x1b[5m";
        pub const BLACK: &str    = "\x1b[30m";
        pub const ORANGE: &str   = "\x1b[38;5;208m";
        pub const PURPLE: &str   = "\x1b[38;5;93m";
        pub const DARK_GRAY: &str   = "\x1b[38;5;238m";
        pub const LIGHT_GRAY: &str  = "\x1b[38;5;245m";
        pub const PINK: &str            = "\x1b[38;5;213m";
        pub const BROWN: &str           = "\x1b[38;5;130m";
        pub const LIGHT_GREEN: &str     = "\x1b[92m";
        pub const LIGHT_BLUE: &str  = "\x1b[94m";
        pub const LIGHT_CYAN: &str  = "\x1b[96m";
        pub const LIGHT_RED: &str   = "\x1b[91m";
        pub const LIGHT_MAGENTA: &str   = "\x1b[95m";
        pub const LIGHT_YELLOW: &str    = "\x1b[93m";
        pub const LIGHT_WHITE: &str     = "\x1b[97m";
    }

    pub mod logging {
        use std::{fs::OpenOptions, io::Write, path::Path};

        // Redirect Pancakes logs to file so the TUI stays clean.
        pub fn write_line(line: &str) {
            let path = std::env::var("PANCAKES_LOG_PATH").unwrap_or_else(|_| "logs/pancakes.log".to_string());
            if let Some(parent) = Path::new(&path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
                let _ = writeln!(f, "{}", line);
            }
        }
    }

    #[macro_export]
    macro_rules! log {
        // colored, raw literal — works with things like "tx: { tx}"
        ($color:expr, $msg:literal) => {{
            let time = chrono::Utc::now().format("%H:%M:%S%.3f").to_string();
            let _ = $crate::writing::logging::write_line(&format!(
                "{} | {}{}{}",
                time,
                $color,
                $msg,
                $crate::writing::cc::RESET,
            ));
        }};

        // colored, with normal formatting: log!(cc::RED, "err: {}", e);
        ($color:expr, $fmt:literal, $($arg:tt)+) => {{
            let time = chrono::Utc::now().format("%H:%M:%S%.3f").to_string();
            let _ = $crate::writing::logging::write_line(&format!(
                "{} | {}{}{}",
                time,
                $color,
                format_args!($fmt, $($arg)+),
                $crate::writing::cc::RESET,
            ));
        }};

        // default color, raw literal
        ($msg:literal) => {{
            let time = chrono::Utc::now().format("%H:%M:%S%.3f").to_string();
            let _ = $crate::writing::logging::write_line(&format!(
                "{} | {}{}",
                time,
                $crate::writing::cc::LIGHT_GRAY,
                $msg,
            ));
        }};

        // default color, with formatting
        ($fmt:literal, $($arg:tt)+) => {{
            let time = chrono::Utc::now().format("%H:%M:%S%.3f").to_string();
            let _ = $crate::writing::logging::write_line(&format!(
                "{} | {}{}{}",
                time,
                $crate::writing::cc::LIGHT_GRAY,
                format_args!($fmt, $($arg)+),
                $crate::writing::cc::RESET,
            ));
        }};
    }
}
pub mod pancake;
pub mod plug;
