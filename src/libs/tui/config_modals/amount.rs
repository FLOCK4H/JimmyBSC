use super::types::{ConfigAreas, ConfigStore};
use crate::libs::tui::draw_inline_input;
use ratatui::{prelude::*, widgets::Paragraph};

pub fn draw_buy_amount(
    f: &mut Frame,
    area: Rect,
    store: &ConfigStore,
    areas: &mut ConfigAreas,
    focused_field: Option<&str>,
) -> u16 {
    let val = store
        .get("buy_amount_wbnb")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "0.00001".to_string());
    draw_inline_input(
        f,
        area,
        "Buy amount: ",
        &val,
        " WBNB",
        focused_field == Some("buy_amount_wbnb"),
    );
    areas.buy_amount_input = Some(area);
    1
}
