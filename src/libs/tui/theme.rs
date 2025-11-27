use ratatui::prelude::*;

#[derive(Clone, Debug)]
pub struct Theme {
    pub bg: Color,
    pub fg: Color,
    pub accent: Color,
    pub accent_soft: Color,
    pub good: Color,
    pub bad: Color,
    pub v2: Color,
    pub v3: Color,
    pub fm: Color,
}

impl Theme {
    pub fn bsc_dark() -> Self {
        Self {
            bg: Color::Black,
            fg: Color::White,
            accent: Color::LightCyan,
            accent_soft: Color::DarkGray,
            good: Color::Green,
            bad: Color::Red,
            v2: Color::Yellow,
            v3: Color::LightMagenta,
            fm: Color::LightGreen,
        }
    }
}
