use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Padding},
};

pub fn draw_main_window(f: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .style(Style::default().bg(Color::Black).fg(Color::White))
        .padding(Padding {
            left: 1,
            right: 1,
            top: 1,
            bottom: 1,
        });
    f.render_widget(block, area);
}
