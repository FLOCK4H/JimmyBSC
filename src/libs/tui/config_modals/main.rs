use super::types::{ConfigAreas, ConfigStore};
use super::{
    advanced::draw_advanced, amount::draw_buy_amount, dexes::draw_dexes, enabled::draw_enabled,
};
use ratatui::{prelude::*, widgets::Paragraph};

/// Draws the Auto Trade configuration modal. This container has NO borders.
/// It lays out several sections vertically and fills `areas_out` with the
/// interactive rectangles for mouse handling.
pub fn draw_config_main(
    f: &mut Frame,
    area: Rect,
    store: &ConfigStore,
    areas_out: &mut ConfigAreas,
    focused_field: Option<&str>,
    scroll_offset: usize,
) -> u16 {
    // Create a scroll-free layout; compute minimal heights by sections
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Enabled
            Constraint::Length(4), // Dexes title + 3
            Constraint::Length(1), // Buy amount
            Constraint::Min(5),    // Advanced (grows)
        ])
        .split(area);

    // Enabled toggle line
    let mut areas = areas_out.clone();
    let _h1 = draw_enabled(f, layout[0], store, &mut areas);
    let _h2 = draw_dexes(f, layout[1], store, &mut areas);
    let _h3 = draw_buy_amount(f, layout[2], store, &mut areas, focused_field);
    let rows_used = draw_advanced(f, layout[3], store, &mut areas, focused_field, scroll_offset);

    *areas_out = areas;
    rows_used
}
