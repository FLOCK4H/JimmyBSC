use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

pub fn draw_toggle(f: &mut Frame, area: Rect, label: &str, on: bool, focused: bool) {
    let (txt, col) = if on {
        ("ON", Color::Green)
    } else {
        ("OFF", Color::Red)
    };
    let line = Line::from(vec![
        Span::styled(
            format!("[ {} ] ", txt),
            Style::default().fg(col).add_modifier(Modifier::BOLD),
        ),
        Span::styled(label, Style::default().fg(Color::White)),
    ]);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused {
            Color::LightCyan
        } else {
            Color::Gray
        }));
    let p = Paragraph::new(line).block(block);
    f.render_widget(p, area);
}
