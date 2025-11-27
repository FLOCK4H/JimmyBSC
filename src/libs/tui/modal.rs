use crate::libs::tui::theme::Theme;
use ratatui::{
    layout::Margin,
    prelude::*,
    widgets::{
        Block, BorderType, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
        StatefulWidget,
    },
};

pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    let horiz = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vert[1]);

    horiz[1]
}

/// Draw a modal that **shrinks** to the longest line passed in `lines`.
/// We don't change the caller's `x` / `y` / `height`, only the width.
pub fn draw_modal(f: &mut Frame, area: Rect, title: &str, lines: &[&str]) {
    let final_width = area.width;
    let modal_area = Rect {
        x: area.x,
        y: area.y,
        width: final_width,
        height: area.height,
    };
    let inner_width = final_width.saturating_sub(2) as usize;
    let content: Vec<Line> = lines
        .iter()
        .enumerate()
        .map(|(idx, s)| {
            let mut txt = s.to_string();
            let missing = inner_width.saturating_sub(txt.chars().count());
            if missing > 0 {
                txt.push_str(&" ".repeat(missing));
            }
            let bg = if idx % 2 == 0 {
                Color::Black
            } else {
                Color::DarkGray
            };
            Line::from(Span::styled(txt, Style::default().fg(Color::White).bg(bg)))
        })
        .collect();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Span::styled(title, Style::default().fg(Color::LightCyan)));
    let p = Paragraph::new(content).block(block);
    f.render_widget(p, modal_area);
}

/// Hermes panel with dense two-line rows, colored tags and scroll highlight.
pub fn draw_modal_pairs(
    f: &mut Frame,
    area: Rect,
    title: &str,
    pairs: &[(String, String, String)],
    vertical_scroll: usize,
    scroll_state: &mut ScrollbarState,
) {
    let theme = Theme::bsc_dark();
    let mut raw_lines: Vec<(String, bool)> = Vec::new();
    if pairs.is_empty() {
        raw_lines.push((
            "Hermes is live. Waiting for incoming v2/v3/fm pairs…".into(),
            true,
        ));
    } else {
        for (idx, (l1, l2, l3)) in pairs.iter().enumerate() {
            let main = format!("{:>2}. {}", idx + 1, l1);
            raw_lines.push((main.clone(), true));
            raw_lines.push((format!("  {}", l2.clone()), false));
            raw_lines.push((format!("  {}", l3.clone()), false));
        }
    }

    let max_len = raw_lines
        .iter()
        .map(|(s, _)| s.chars().count())
        .max()
        .unwrap_or(0);
    let target_width: u16 = ((max_len + 4) as u16).min(area.width);
    let modal_area = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: area.height,
    };
    let inner_width = target_width.saturating_sub(2) as usize;

    let mut content: Vec<Line> = Vec::with_capacity(raw_lines.len());
    for (i, (s, is_first)) in raw_lines.into_iter().enumerate() {
        let mut t = s;
        let miss = inner_width.saturating_sub(t.chars().count());
        if miss > 0 {
            t.push_str(&" ".repeat(miss));
        }
        let is_selected = i == vertical_scroll;
        let base_style = if is_first {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::Gray)
        };
        let style = if is_selected {
            base_style.bg(Color::DarkGray).add_modifier(Modifier::BOLD)
        } else {
            base_style
        };

        // Parse and color PnL segment if present (now on second line in triplet)
        if t.contains("| PnL:") {
            if let Some(pnl_start) = t.find("| PnL:") {
                let after_pnl_start = pnl_start + "| PnL:".len();

                // Find the end of the PnL value (next | or end of string before " | Price:")
                let pnl_end = if let Some(price_idx) = t[after_pnl_start..].find(" | Price:") {
                    after_pnl_start + price_idx
                } else {
                    t.len()
                };

                let before_pnl = t[..pnl_start].to_string() + "| PnL:";
                let pnl_segment = t[after_pnl_start..pnl_end].to_string();
                let after_pnl = t[pnl_end..].to_string();

                // Determine color based on + or - sign
                let pnl_color = if pnl_segment.trim_start().starts_with('+') {
                    Color::Green
                } else if pnl_segment.trim_start().starts_with('-') {
                    Color::Red
                } else {
                    Color::Gray
                };

                let pnl_style = if is_selected {
                    Style::default()
                        .fg(pnl_color)
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(pnl_color)
                };

                let mut spans = vec![
                    Span::styled(before_pnl, style),
                    Span::styled(pnl_segment, pnl_style),
                ];

                if !after_pnl.is_empty() {
                    spans.push(Span::styled(after_pnl, style));
                }

                content.push(Line::from(spans));
            } else {
                content.push(Line::from(Span::styled(t, style)));
            }
        } else {
            content.push(Line::from(Span::styled(t, style)));
        }
    }

    let block_title = format!("{} ({})", title, pairs.len());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Span::styled(block_title, Style::default().fg(theme.accent)));

    let content_len = content.len();
    let viewport_len_text = modal_area.height.saturating_sub(2) as usize;
    let scroll_pos = vertical_scroll.min(content_len.saturating_sub(viewport_len_text));
    let p = Paragraph::new(content.clone())
        .scroll((scroll_pos as u16, 0))
        .block(block);
    f.render_widget(p, modal_area);

    // Only render scrollbar if there's something to scroll
    if content_len > viewport_len_text {
        let mut s = core::mem::take(scroll_state);
        // Track height equals inner(height)-2 for our margins
        let viewport_for_scroll = modal_area.inner(Margin::new(0, 1)).height as usize;
        s = s
            .content_length(content_len)
            .viewport_content_length(viewport_for_scroll)
            .position(scroll_pos);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("↑"))
                .end_symbol(Some("↓")),
            modal_area.inner(Margin::new(0, 1)),
            &mut s,
        );
        *scroll_state = s;
    }
}

/// Scrollable list of lines (used by Orb). Two-line entries can be pushed by caller.
pub fn draw_modal_lines(
    f: &mut Frame,
    area: Rect,
    title: &str,
    lines: &[String],
    vertical_scroll: usize,
    scroll_state: &mut ScrollbarState,
) {
    let final_width = area.width;
    let modal_area = Rect {
        x: area.x,
        y: area.y,
        width: final_width,
        height: area.height,
    };
    let inner_width = final_width.saturating_sub(2) as usize;

    let mut content: Vec<Line> = Vec::new();
    if lines.is_empty() {
        let placeholders = ["…", "…"];
        for (i, s) in placeholders.iter().enumerate() {
            let mut t = s.to_string();
            let miss = inner_width.saturating_sub(t.chars().count());
            if miss > 0 {
                t.push_str(&" ".repeat(miss));
            }
            let is_first = i % 2 == 0;
            let base_style = if is_first {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::Gray)
            };
            content.push(Line::from(Span::styled(t, base_style)));
        }
    } else {
        content.reserve(lines.len());
        for (i, s) in lines.iter().enumerate() {
            let mut t = s.clone();
            let miss = inner_width.saturating_sub(t.chars().count());
            if miss > 0 {
                t.push_str(&" ".repeat(miss));
            }
            let is_first = i % 2 == 0;
            let base_style = if is_first {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::Gray)
            };
            content.push(Line::from(Span::styled(t, base_style)));
        }
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Span::styled(title, Style::default().fg(Color::LightCyan)));

    let content_len = content.len();
    let viewport_len_text = modal_area.height.saturating_sub(2) as usize;
    let scroll_pos = vertical_scroll.min(content_len.saturating_sub(viewport_len_text));
    let p = Paragraph::new(content.clone())
        .scroll((scroll_pos as u16, 0))
        .block(block);
    f.render_widget(p, modal_area);

    if content_len > viewport_len_text {
        let mut s = core::mem::take(scroll_state);
        let viewport_for_scroll = modal_area.inner(Margin::new(0, 1)).height as usize;
        s = s
            .content_length(content_len)
            .viewport_content_length(viewport_for_scroll)
            .position(scroll_pos);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("↑"))
                .end_symbol(Some("↓")),
            modal_area.inner(Margin::new(0, 1)),
            &mut s,
        );
        *scroll_state = s;
    }
}
