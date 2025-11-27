use super::types::{ConfigAreas, ConfigStore};
use ratatui::{prelude::*, widgets::Paragraph};

fn draw_checkbox_line(f: &mut Frame, area: Rect, label: &str, checked: bool) {
    let mark = if checked { "[x]" } else { "[ ]" };
    let line = Line::from(vec![
        Span::styled(mark, Style::default().fg(Color::LightCyan)),
        Span::raw(" "),
        Span::styled(label, Style::default().fg(Color::White)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

pub fn draw_dexes(f: &mut Frame, area: Rect, store: &ConfigStore, areas: &mut ConfigAreas) -> u16 {
    // Title
    let title = Line::from(Span::styled(
        "Selected dexes:",
        Style::default().fg(Color::Gray),
    ));
    f.render_widget(
        Paragraph::new(title),
        Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1,
        },
    );

    let selected = store
        .get("dexes")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "v2,v3,fm".to_string());
    let has = |k: &str| selected.split(',').any(|s| s.trim() == k);
    let v2 = has("v2");
    let v3 = has("v3");
    let fm = has("fm");

    // three rows
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width,
            height: area.height.saturating_sub(1),
        });

    draw_checkbox_line(f, rows[0], "v2", v2);
    draw_checkbox_line(f, rows[1], "v3", v3);
    draw_checkbox_line(f, rows[2], "fm", fm);

    areas.dexes = Some(vec![rows[0], rows[1], rows[2]]);

    4 // title + 3 rows
}
