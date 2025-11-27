use std::io::{self, StdoutLock, Write};

pub mod cc {
    pub const RED: &str = "\x1b[31m";
    pub const GREEN: &str = "\x1b[32m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const BLUE: &str = "\x1b[34m";
    pub const MAGENTA: &str = "\x1b[35m";
    pub const CYAN: &str = "\x1b[36m";
    pub const WHITE: &str = "\x1b[37m";
    pub const BOLD: &str = "\x1b[1m";
    pub const RESET: &str = "\x1b[0m";
    pub const BLINK: &str = "\x1b[5m";
    pub const BLACK: &str = "\x1b[30m";
    pub const ORANGE: &str = "\x1b[38;5;208m";
    pub const PURPLE: &str = "\x1b[38;5;93m";
    pub const DARK_GRAY: &str = "\x1b[38;5;238m";
    pub const LIGHT_GRAY: &str = "\x1b[38;5;245m";
    pub const PINK: &str = "\x1b[38;5;213m";
    pub const BROWN: &str = "\x1b[38;5;130m";
    pub const LIGHT_GREEN: &str = "\x1b[92m";
    pub const LIGHT_BLUE: &str = "\x1b[94m";
    pub const LIGHT_CYAN: &str = "\x1b[96m";
    pub const LIGHT_RED: &str = "\x1b[91m";
    pub const LIGHT_MAGENTA: &str = "\x1b[95m";
    pub const LIGHT_YELLOW: &str = "\x1b[93m";
    pub const LIGHT_WHITE: &str = "\x1b[97m";
}

#[macro_export]
macro_rules! log {
    // -----------------------------------------------------------------
    // 1) colored, no extra args
    //    log!(cc::RED, "hello");
    // -----------------------------------------------------------------
    ($color:expr, $fmt:literal $(,)?) => {{
        let time = chrono::Utc::now().format("%H:%M:%S%.3f").to_string();
        let mut _stderr = ::std::io::stderr().lock();
        let _ = ::std::io::Write::write_fmt(
            &mut _stderr,
            format_args!(
                concat!("{}{} | {}", "{}", $fmt, "{}", "\n"),
                $crate::libs::writing::cc::LIGHT_GRAY,
                time,
                $crate::libs::writing::cc::RESET,
                $color,
                $crate::libs::writing::cc::RESET,
            ),
        );
    }};

    // -----------------------------------------------------------------
    // 2) colored, with args
    //    log!(cc::GREEN, "swap: {} -> {}", a, b);
    // -----------------------------------------------------------------
    ($color:expr, $fmt:literal, $($arg:tt)+ $(,)?) => {{
        let time = chrono::Utc::now().format("%H:%M:%S%.3f").to_string();
        let mut _stderr = ::std::io::stderr().lock();
        let _ = ::std::io::Write::write_fmt(
            &mut _stderr,
            format_args!(
                concat!("{}{} | {}", "{}", $fmt, "{}", "\n"),
                $crate::libs::writing::cc::LIGHT_GRAY,
                time,
                $crate::libs::writing::cc::RESET,
                $color,
                $($arg)+,
                $crate::libs::writing::cc::RESET,
            ),
        );
    }};

    // -----------------------------------------------------------------
    // 3) default color, no args
    //    log!("hello");
    // -----------------------------------------------------------------
    ($fmt:literal $(,)?) => {{
        let time = chrono::Utc::now().format("%H:%M:%S%.3f").to_string();
        let mut _stderr = ::std::io::stderr().lock();
        let _ = ::std::io::Write::write_fmt(
            &mut _stderr,
            format_args!(
                concat!("{}{} | {}", "{}", $fmt, "{}", "\n"),
                $crate::libs::writing::cc::LIGHT_GRAY,
                time,
                $crate::libs::writing::cc::RESET,
                $crate::libs::writing::cc::LIGHT_GRAY,
                $crate::libs::writing::cc::RESET,
            ),
        );
    }};

    // -----------------------------------------------------------------
    // 4) default color, with args
    //    log!("price: {}", p);
    // -----------------------------------------------------------------
    ($fmt:literal, $($arg:tt)+ $(,)?) => {{
        let time = chrono::Utc::now().format("%H:%M:%S%.3f").to_string();
        let mut _stderr = ::std::io::stderr().lock();
        let _ = ::std::io::Write::write_fmt(
            &mut _stderr,
            format_args!(
                concat!("{}{} | {}", "{}", $fmt, "{}", "\n"),
                $crate::libs::writing::cc::LIGHT_GRAY,
                time,
                $crate::libs::writing::cc::RESET,
                $crate::libs::writing::cc::LIGHT_GRAY,
                $($arg)+,
                $crate::libs::writing::cc::RESET,
            ),
        );
    }};
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {{
        let mut _stderr = ::std::io::stderr().lock();
        let _ = ::std::io::Write::write_fmt(
            &mut _stderr,
            format_args!(
                "{}{}{}",
                $crate::libs::writing::cc::ORANGE,
                format_args!($($arg)*),
                $crate::libs::writing::cc::RESET,
            ),
        );
        let _ = ::std::io::Write::write_fmt(&mut _stderr, format_args!("\n"));
    }};
}

pub struct Colors<'a> {
    lock: StdoutLock<'a>,
}

impl<'a> Colors<'a> {
    pub fn new(lock: StdoutLock<'a>) -> Self {
        Self { lock }
    }

    pub fn cprint(&mut self, text: &str, color: &str) {
        let _ = writeln!(self.lock, "{}{}{}", color, text, cc::RESET);
    }

    pub fn cinput(&mut self, text: &str, color: &str) -> String {
        let _ = writeln!(self.lock, "{}{}{}", color, text, cc::RESET);
        let mut input = String::new();
        let _ = io::stdin().read_line(&mut input);
        input.trim().to_string()
    }

    pub fn err_print(&mut self, text: &str) {
        let _ = writeln!(self.lock, "{}{}{}", cc::RED, text, cc::RESET);
    }
}

#[cfg(test)]
mod tests {
    // we are *inside* jimmyb, so we can reach the macros
    #[test]
    fn smoke_log_variants_compile() {
        crate::log!(crate::libs::writing::cc::GREEN, "colored no args");
        crate::log!(crate::libs::writing::cc::GREEN, "colored with arg: {}", 123);
        crate::log!("plain no args");
        crate::log!("plain with arg: {}", 456);
    }

    #[test]
    fn smoke_colors() {
        let out = std::io::stdout();
        let lock = out.lock();
        let mut c = crate::libs::writing::Colors::new(lock);
        c.cprint("hello", crate::libs::writing::cc::BLUE);
        c.err_print("err");
    }
}
