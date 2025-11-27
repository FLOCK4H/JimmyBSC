use super::types::{ConfigAreas, ConfigStore};
use ratatui::{prelude::*, widgets::Paragraph};

pub fn draw_enabled(
    f: &mut Frame,
    area: Rect,
    store: &ConfigStore,
    areas: &mut ConfigAreas,
) -> u16 {
    let on = store
        .get("enabled")
        .map(|v| v.as_str() == "true")
        .unwrap_or(false);
    let status = if on {
        Span::styled(
            "ON",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            "OFF",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )
    };
    let line = Line::from(vec![
        Span::styled("Enabled: ", Style::default().fg(Color::Gray)),
        status,
        Span::raw("    (click to toggle)"),
    ]);
    f.render_widget(Paragraph::new(line), area);
    areas.enabled_btn = Some(area);
    1
}
