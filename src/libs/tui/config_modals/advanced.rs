use super::types::{ConfigAreas, ConfigStore};
use crate::libs::tui::draw_inline_input;
use crate::shared::ALL_QUOTES;
use ratatui::{prelude::*, widgets::Paragraph};

fn kv<'a>(store: &'a ConfigStore, key: &str, default: &str) -> String {
    store
        .get(key)
        .map(|v| v.to_string())
        .unwrap_or_else(|| default.to_string())
}

fn draw_line(f: &mut Frame, area: Rect, left: &str, val: &str, suffix: &str, focused: bool) {
    draw_inline_input(f, area, left, val, suffix, focused);
}

fn draw_checkbox_line(f: &mut Frame, area: Rect, label: &str, checked: bool) {
    let mark = if checked { "[x]" } else { "[ ]" };
    let line = Line::from(vec![
        Span::styled(mark, Style::default().fg(Color::LightCyan)),
        Span::raw(" "),
        Span::styled(label, Style::default().fg(Color::White)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

/// Draw the advanced config with manual scrolling. Rows are 1 line tall; we only
/// render the visible slice [start, end).
pub fn draw_advanced(
    f: &mut Frame,
    area: Rect,
    store: &ConfigStore,
    areas: &mut ConfigAreas,
    focused_field: Option<&str>,
    scroll_offset: usize,
) -> u16 {
    let total_rows = 18usize;
    let viewport = area.height.max(1) as usize;
    let max_start = total_rows.saturating_sub(1);
    let start = scroll_offset.min(max_start);
    let end = (start + viewport).min(total_rows);

    let mut accepted_quotes_areas = Vec::new();

    for row_index in start..end {
        let row_rect = Rect {
            x: area.x,
            y: area.y + (row_index - start) as u16,
            width: area.width,
            height: 1,
        };

        match row_index {
            0 => {
                draw_line(
                    f,
                    row_rect,
                    "Slippage: ",
                    &kv(store, "slippage_pct", "0.5"),
                    "%",
                    focused_field == Some("slippage_pct"),
                );
                areas.slippage_input = Some(row_rect);
            }
            1 => {
                draw_line(
                    f,
                    row_rect,
                    "Max gas: ",
                    &kv(store, "max_gwei", "1.0"),
                    " gwei",
                    focused_field == Some("max_gwei"),
                );
                areas.max_gwei_input = Some(row_rect);
            }
            2 => {
                draw_line(
                    f,
                    row_rect,
                    "Max positions: ",
                    &kv(store, "max_positions", "3"),
                    "",
                    focused_field == Some("max_positions"),
                );
                areas.max_positions_input = Some(row_rect);
            }
            3 => {
                draw_line(
                    f,
                    row_rect,
                    "Min liquidity: $",
                    &kv(store, "min_liquidity", "1000"),
                    "",
                    focused_field == Some("min_liquidity"),
                );
                areas.min_liq_input = Some(row_rect);
            }
            4 => {
                draw_line(
                    f,
                    row_rect,
                    "Min buys: ",
                    &kv(store, "min_buys", "3"),
                    "",
                    focused_field == Some("min_buys"),
                );
                areas.min_buys_input = Some(row_rect);
            }
            5 => {
                draw_line(
                    f,
                    row_rect,
                    "Max hold: ",
                    &kv(store, "max_hold_secs", "0"),
                    " s",
                    focused_field == Some("max_hold_secs"),
                );
                areas.max_hold_input = Some(row_rect);
            }
            6 => {
                let mh_pnl_en = store
                    .get("max_hold_pnl")
                    .map(|v| v.as_str() == "true")
                    .unwrap_or(true);
                draw_checkbox_line(f, row_rect, "Max Hold PnL", mh_pnl_en);
                areas.max_hold_pnl_toggle = Some(row_rect);
            }
            7 => {
                let tp_en = store
                    .get("tp_enabled")
                    .map(|v| v.as_str() == "true")
                    .unwrap_or(false);
                draw_checkbox_line(f, row_rect, "Take profit", tp_en);
                areas.tp_toggle = Some(row_rect);
            }
            8 => {
                draw_line(
                    f,
                    row_rect,
                    "  Target: ",
                    &kv(store, "tp_pct", "10"),
                    "%",
                    focused_field == Some("tp_pct"),
                );
                areas.tp_pct_input = Some(row_rect);
            }
            9 => {
                let sl_en = store
                    .get("sl_enabled")
                    .map(|v| v.as_str() == "true")
                    .unwrap_or(false);
                draw_checkbox_line(f, row_rect, "Stop loss", sl_en);
                areas.sl_toggle = Some(row_rect);
            }
            10 => {
                draw_line(
                    f,
                    row_rect,
                    "  Trigger: ",
                    &kv(store, "sl_pct", "5"),
                    "%",
                    focused_field == Some("sl_pct"),
                );
                areas.sl_pct_input = Some(row_rect);
            }
            11 => {
                let label = Line::from(Span::styled(
                    "Accepted Quotes (click to toggle)",
                    Style::default().fg(Color::Gray),
                ));
                f.render_widget(Paragraph::new(label), row_rect);
            }
            12 | 13 => {
                let selected_csv = store
                    .get("accepted_quotes")
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "BNB,CAKE,USDT,USD1,ASTER,WBNB".to_string());
                let grid_row = row_index - 12;
                let base_idx = grid_row * 3;
                let cols = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Ratio(1, 3),
                        Constraint::Ratio(1, 3),
                        Constraint::Ratio(1, 3),
                    ])
                    .split(row_rect);
                for (i, c) in cols.iter().enumerate() {
                    let idx = base_idx + i;
                    if idx >= ALL_QUOTES.len() {
                        break;
                    }
                    let quote = ALL_QUOTES[idx];
                    let checked = selected_csv.split(',').any(|s| s.trim() == quote);
                    draw_checkbox_line(f, *c, quote, checked);
                    accepted_quotes_areas.push(*c);
                }
            }
            14 => {
                draw_line(
                    f,
                    row_rect,
                    "Wrap ratio: ",
                    &kv(store, "wrap_ratio_pct", "80"),
                    "%",
                    focused_field == Some("wrap_ratio_pct"),
                );
                areas.wrap_ratio_input = Some(row_rect);
            }
            15 => {
                let avoid_cn = store
                    .get("avoid_chinese")
                    .map(|v| v.as_str() == "true")
                    .unwrap_or(false);
                draw_checkbox_line(f, row_rect, "Avoid Chinese", avoid_cn);
                areas.avoid_chinese_toggle = Some(row_rect);
            }
            16 => {
                draw_line(
                    f,
                    row_rect,
                    "Freshness: ",
                    &kv(store, "freshness_secs", "30"),
                    " s",
                    focused_field == Some("freshness_secs"),
                );
                areas.freshness_input = Some(row_rect);
            }
            17 => {
                draw_line(
                    f,
                    row_rect,
                    "Min PnL: ",
                    &kv(store, "min_pnl_pct", "100"),
                    " %",
                    focused_field == Some("min_pnl_pct"),
                );
                areas.min_pnl_input = Some(row_rect);
            }
            _ => {}
        }
    }

    areas.accepted_quotes = Some(accepted_quotes_areas);

    total_rows as u16
}
