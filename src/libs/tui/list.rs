use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState},
};

pub fn draw_list(
    f: &mut Frame,
    area: Rect,
    items: &[impl AsRef<str>],
    state: &mut ListState,
    title: &str,
) {
    let list_items: Vec<ListItem> = items
        .iter()
        .map(|i| ListItem::new(i.as_ref().to_string()))
        .collect();
    let list = List::new(list_items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    f.render_stateful_widget(list, area, state);
}

pub fn list_next(state: &mut ListState, len: usize) {
    let i = match state.selected() {
        Some(i) => {
            if i + 1 >= len {
                0
            } else {
                i + 1
            }
        }
        None => 0,
    };
    state.select(Some(i));
}

pub fn list_prev(state: &mut ListState, len: usize) {
    let i = match state.selected() {
        Some(i) => {
            if i == 0 {
                len.saturating_sub(1)
            } else {
                i - 1
            }
        }
        None => 0,
    };
    state.select(Some(i));
}
