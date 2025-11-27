use ratatui::{prelude::*, widgets::Clear};

/// Draw a simple browser-like tab strip (one row, compact).
/// - `hovered`: Some(i) highlights tab i on hover
/// - `active`: currently selected tab index
/// - `out_areas`: filled with per-tab hit rects (for mouse handling)
pub fn draw_tab_strip(
    f: &mut Frame,
    area: Rect,
    labels: &[&str],
    hovered: Option<usize>,
    active: usize,
    out_areas: &mut Vec<Rect>,
) {
    // compact: width per tab = label width + 4 padding
    let widths: Vec<Constraint> = labels
        .iter()
        .map(|s| Constraint::Length(s.chars().count() as u16 + 4))
        .collect();

    let row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(widths)
        .split(area);

    out_areas.clear();
    for (i, rect) in row.iter().copied().enumerate() {
        out_areas.push(rect);
        let is_hover = hovered == Some(i);
        let is_active = active == i;

        // Foreground only; leave background transparent to avoid a painted bar
        let fg = if is_active {
            Color::Cyan
        } else if is_hover {
            Color::LightBlue
        } else {
            Color::White
        };

        let label = format!(" {} ", labels[i]);
        let style = Style::default()
            .fg(fg)
            .add_modifier(if is_active { Modifier::BOLD } else { Modifier::empty() });
        let p = ratatui::widgets::Paragraph::new(Line::from(Span::styled(label, style)))
            .alignment(Alignment::Center);
        f.render_widget(p, rect);
    }
}
