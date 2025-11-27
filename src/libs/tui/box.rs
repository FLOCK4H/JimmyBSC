use ratatui::{
    layout::Margin,
    prelude::*,
    text::Line,
    widgets::{Block, BorderType, Borders, Paragraph},
};

#[derive(Clone)]
pub struct BoxProps {
    pub offset: (u16, u16),
    pub size: (u16, u16),
    pub border_color: Color,
    pub title: String,
}

impl Default for BoxProps {
    fn default() -> Self {
        Self {
            offset: (0, 0),
            size: (30, 4),
            border_color: Color::LightBlue,
            title: "Box".into(),
        }
    }
}

pub fn draw_box(f: &mut Frame, area: Rect, lines: Vec<String>, props: &BoxProps) {
    let margin_h: u16 = 1;
    let margin_v: u16 = 0;
    let needed_h = (lines.len() as u16).saturating_add(2).max(3);
    let max_w = area.width.saturating_sub(props.offset.0);
    let max_h = area.height.saturating_sub(props.offset.1);
    let mut outer = Rect {
        x: area.x.saturating_add(props.offset.0),
        y: area.y.saturating_add(props.offset.1),
        width: max_w,
        height: needed_h.min(max_h),
    };
    if props.size.0 > 0 {
        outer.width = props.size.0.min(max_w);
    }
    if props.size.1 > 0 {
        outer.height = props.size.1.min(max_h).max(needed_h);
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(props.border_color))
        .title(Span::styled(
            format!(" {} ", props.title),
            Style::default()
                .fg(props.border_color)
                .add_modifier(Modifier::BOLD),
        ));
    f.render_widget(block, outer);

    let inner = outer.inner(Margin::new(margin_h, margin_v));
    let rendered: Vec<Line> = lines.into_iter().map(Line::from).collect();
    let para = Paragraph::new(rendered);
    f.render_widget(para, inner);
}
