use ratatui::{
    layout::Margin,
    prelude::*,
    widgets::{Block, Clear, Paragraph},
};

pub fn draw_button(f: &mut Frame, area: Rect, label: &str, focused: bool) {
    if focused {
        // Clear underlying content, then render centered white label
        f.render_widget(Clear, area);
        let p = Paragraph::new(Line::from(Span::styled(
            label,
            Style::default().fg(Color::White).bg(Color::DarkGray),
        )))
        .alignment(Alignment::Center);
        f.render_widget(p, area);
    } else {
        f.render_widget(Clear, area);
        let p = Paragraph::new(Line::from(Span::styled(
            label,
            Style::default().fg(Color::White),
        )))
        .alignment(Alignment::Center)
        .block(Block::default());
        f.render_widget(p, area);
    }
}
