use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

pub fn draw_input(f: &mut Frame, area: Rect, label: &str, value: &str, focused: bool) {
    let line = Line::from(vec![
        Span::styled(format!("{}: ", label), Style::default().fg(Color::Gray)),
        Span::styled(value, Style::default().fg(Color::White)),
        Span::raw(if focused { "_" } else { "" }),
    ]);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused {
            Color::LightCyan
        } else {
            Color::Gray
        }));
    f.render_widget(Paragraph::new(line).block(block), area);
}

/// Draw an inline editable field (no borders, clickable)
pub fn draw_inline_input(
    f: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    suffix: &str,
    focused: bool,
) {
    let line = Line::from(vec![
        Span::styled(label, Style::default().fg(Color::Gray)),
        Span::styled(
            value,
            Style::default()
                .fg(if focused {
                    Color::LightCyan
                } else {
                    Color::White
                })
                .add_modifier(if focused {
                    Modifier::UNDERLINED
                } else {
                    Modifier::empty()
                }),
        ),
        Span::styled(suffix, Style::default().fg(Color::Gray)),
        Span::raw(if focused { " _" } else { "" }),
    ]);
    f.render_widget(Paragraph::new(line), area);
}
