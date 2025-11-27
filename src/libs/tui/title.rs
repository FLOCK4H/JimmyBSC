use crate::libs::tui::theme::Theme;
use ratatui::{
    prelude::*,
    text::{Line, Span},
    widgets::{Block, Paragraph},
};

pub fn draw_title_bar(
    f: &mut Frame,
    area: Rect,
    app_name: &str,
    mid_status: &str,
    right_help: &str,
) {
    let theme = Theme::bsc_dark();
    let bar = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: 3,
    };

    let left = Span::styled(
        format!(" {} ", app_name),
        Style::default()
            .fg(theme.bg)
            .bg(theme.accent)
            .add_modifier(Modifier::BOLD),
    );
    let center = Span::styled(
        format!("  {}  ", mid_status),
        Style::default().fg(Color::Gray),
    );
    let right = Span::styled(
        right_help,
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::ITALIC),
    );

    let line = Line::from(vec![left, Span::raw("  "), center, Span::raw("  "), right]);
    let p = Paragraph::new(line)
        .alignment(Alignment::Center)
        .block(Block::new().style(Style::default().bg(theme.bg)));
    f.render_widget(p, bar);
}
