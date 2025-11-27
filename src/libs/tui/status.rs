use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

pub fn draw_status(f: &mut Frame, area: Rect, text: &str, ok: bool) {
    let color = if ok { Color::Green } else { Color::Red };
    let p = Paragraph::new(Span::styled(text, Style::default().fg(color)))
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(p, area);
}
