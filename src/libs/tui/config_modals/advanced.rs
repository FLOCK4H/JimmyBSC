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

pub fn draw_advanced(
    f: &mut Frame,
    area: Rect,
    store: &ConfigStore,
    areas: &mut ConfigAreas,
    focused_field: Option<&str>,
) -> u16 {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // slippage
            Constraint::Length(1), // max gwei
            Constraint::Length(1), // max positions
            Constraint::Length(1), // min liquidity
            Constraint::Length(1), // min buys
            Constraint::Length(1), // max hold secs
            Constraint::Length(1), // max hold pnl toggle
            Constraint::Length(1), // tp toggle
            Constraint::Length(1), // tp pct
            Constraint::Length(1), // sl toggle
            Constraint::Length(1), // sl pct
            Constraint::Length(1), // accepted quotes label
            Constraint::Length(1), // quotes row 1
            Constraint::Length(1), // quotes row 2
        ])
        .split(area);

    draw_line(
        f,
        rows[0],
        "Slippage: ",
        &kv(store, "slippage_pct", "0.5"),
        "%",
        focused_field == Some("slippage_pct"),
    );
    areas.slippage_input = Some(rows[0]);
    draw_line(
        f,
        rows[1],
        "Max gas: ",
        &kv(store, "max_gwei", "1.0"),
        " gwei",
        focused_field == Some("max_gwei"),
    );
    areas.max_gwei_input = Some(rows[1]);
    draw_line(
        f,
        rows[2],
        "Max positions: ",
        &kv(store, "max_positions", "3"),
        "",
        focused_field == Some("max_positions"),
    );
    areas.max_positions_input = Some(rows[2]);

    // New fields for filtering
    draw_line(
        f,
        rows[3],
        "Min liquidity: $",
        &kv(store, "min_liquidity", "1000"),
        "",
        focused_field == Some("min_liquidity"),
    );
    areas.min_liq_input = Some(rows[3]);
    draw_line(
        f,
        rows[4],
        "Min buys: ",
        &kv(store, "min_buys", "3"),
        "",
        focused_field == Some("min_buys"),
    );
    areas.min_buys_input = Some(rows[4]);

    // Max hold in seconds (0 = disabled)
    draw_line(
        f,
        rows[5],
        "Max hold: ",
        &kv(store, "max_hold_secs", "0"),
        " s",
        focused_field == Some("max_hold_secs"),
    );
    areas.max_hold_input = Some(rows[5]);

    // Max Hold PnL checkbox (controls applying -50% threshold)
    let mh_pnl_en = store
        .get("max_hold_pnl")
        .map(|v| v.as_str() == "true")
        .unwrap_or(true);
    draw_checkbox_line(f, rows[6], "Max Hold PnL", mh_pnl_en);
    areas.max_hold_pnl_toggle = Some(rows[6]);

    let tp_en = store
        .get("tp_enabled")
        .map(|v| v.as_str() == "true")
        .unwrap_or(false);
    draw_checkbox_line(f, rows[7], "Take profit", tp_en);
    areas.tp_toggle = Some(rows[7]);
    draw_line(
        f,
        rows[8],
        "  Target: ",
        &kv(store, "tp_pct", "10"),
        "%",
        focused_field == Some("tp_pct"),
    );
    areas.tp_pct_input = Some(rows[8]);

    let sl_en = store
        .get("sl_enabled")
        .map(|v| v.as_str() == "true")
        .unwrap_or(false);
    draw_checkbox_line(f, rows[9], "Stop loss", sl_en);
    areas.sl_toggle = Some(rows[9]);
    draw_line(
        f,
        rows[10],
        "  Trigger: ",
        &kv(store, "sl_pct", "5"),
        "%",
        focused_field == Some("sl_pct"),
    );
    areas.sl_pct_input = Some(rows[10]);

    // Accepted quotes: label + 2x3 grid
    let label_area = rows[11];
    let label = Line::from(Span::styled(
        "Accepted Quotes (click to toggle)",
        Style::default().fg(Color::Gray),
    ));
    f.render_widget(Paragraph::new(label), label_area);

    let selected_csv = store
        .get("accepted_quotes")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "BNB,CAKE,USDT,USD1,ASTER,WBNB".to_string());

    let mut accepted_quotes_areas = Vec::new();
    let grid_rows = [rows[12], rows[13]]; // 2 rows for the grid
    let col_constraints = [
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
    ];

    let mut idx = 0usize;
    for gr in grid_rows.iter() {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(col_constraints)
            .split(*gr);
        for c in cols.iter() {
            if idx >= ALL_QUOTES.len() {
                break;
            }
            let quote = ALL_QUOTES[idx];
            let checked = selected_csv.split(',').any(|s| s.trim() == quote);
            draw_checkbox_line(f, *c, quote, checked);
            accepted_quotes_areas.push(*c);
            idx += 1;
        }
    }

    areas.accepted_quotes = Some(accepted_quotes_areas);

    14 // Total rows used
}
