#![warn(unused)]
use {
    crate::libs::lookup::save_log_to_file,
    crate::libs::sim::{DexType, SimEngine},
    crate::libs::tui::ConfigStore,
    ratatui::prelude::*,
    ratatui::widgets::ScrollbarState,
    std::collections::HashSet,
};

#[derive(Clone, Debug, Default)]
pub struct ResultsAreas {
    pub take_all_btn: Option<Rect>,
    // (rect, pair_address)
    pub take_btns: Vec<(Rect, String)>,
    // (rect, pair_address)
    pub freeze_btns: Vec<(Rect, String)>,
    // (rect, pair_address)
    pub remove_btns: Vec<(Rect, String)>,
    // (rect, pair_address, fraction)
    pub partial_btns: Vec<(Rect, String, f64)>,
}

pub fn results<'a>(
    f: &mut Frame<'a>,
    area: Rect,
    results: &mut ResultsAreas,
    sim_mode: bool,
    sim_engine: &SimEngine,
    mut results_scroll: usize,
    results_scroll_state: &mut ScrollbarState,
    config_store: &ConfigStore,
    session_secs: u64,
) {
    let stats = sim_engine.stats();
    let open_pos = sim_engine.open_positions();

    // Build lines incrementally while tracking line indices
    let mut stats_lines: Vec<Line> = Vec::new();
    let mut line_idx: usize = 0;

    if !sim_mode {
        stats_lines.push(Line::from(Span::styled(
            "you have disabled sim mode, the app will use your funds to make swaps",
            Style::default().fg(Color::Red),
        )));
        line_idx += 1;
        stats_lines.push(Line::from(""));
        line_idx += 1;
    }

    stats_lines.push(Line::from(Span::styled(
        "═══ Statistics ═══",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    line_idx += 1;
    stats_lines.push(Line::from(""));
    line_idx += 1;

    stats_lines.push(Line::from(vec![
        Span::styled("Total Trades: ", Style::default().fg(Color::Gray)),
        Span::styled(
            format!("{}", stats.total_trades),
            Style::default().fg(Color::White),
        ),
    ]));
    line_idx += 1;

    stats_lines.push(Line::from(vec![
        Span::styled("Winning: ", Style::default().fg(Color::Gray)),
        Span::styled(
            format!("{}", stats.winning_trades),
            Style::default().fg(Color::Green),
        ),
        Span::styled(" | Losing: ", Style::default().fg(Color::Gray)),
        Span::styled(
            format!("{}", stats.losing_trades),
            Style::default().fg(Color::Red),
        ),
    ]));
    line_idx += 1;

    stats_lines.push(Line::from(vec![
        Span::styled("Win Rate: ", Style::default().fg(Color::Gray)),
        Span::styled(
            format!("{:.1}%", stats.win_rate),
            Style::default().fg(Color::Yellow),
        ),
    ]));
    line_idx += 1;

    stats_lines.push(Line::from(""));
    line_idx += 1;

    stats_lines.push(Line::from(vec![
        Span::styled("Total PnL (Closed): ", Style::default().fg(Color::Gray)),
        Span::styled(
            format!("{:+.6} WBNB", stats.total_pnl_closed),
            Style::default().fg(if stats.total_pnl_closed >= 0.0 {
                Color::Green
            } else {
                Color::Red
            }),
        ),
    ]));
    line_idx += 1;
    // Include realized PnL from partial sells so totals update immediately after partial actions
    stats_lines.push(Line::from(vec![
        Span::styled("Total PnL (Realized): ", Style::default().fg(Color::Gray)),
        Span::styled(
            format!("{:+.6} WBNB", stats.total_pnl_realized),
            Style::default().fg(if stats.total_pnl_realized >= 0.0 {
                Color::Green
            } else {
                Color::Red
            }),
        ),
    ]));
    line_idx += 1;

    stats_lines.push(Line::from(vec![
        Span::styled("Avg PnL/Trade: ", Style::default().fg(Color::Gray)),
        Span::styled(
            format!("{:+.6} WBNB", stats.avg_pnl_per_trade),
            Style::default().fg(if stats.avg_pnl_per_trade >= 0.0 {
                Color::Green
            } else {
                Color::Red
            }),
        ),
    ]));
    line_idx += 1;

    // Show current Buy Amount from Auto Trade config
    let buy_amount_str = config_store
        .get("buy_amount_wbnb")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "0.00001".to_string());
    stats_lines.push(Line::from(vec![
        Span::styled("Buy Amount: ", Style::default().fg(Color::Gray)),
        Span::styled(
            format!("{} WBNB", buy_amount_str),
            Style::default().fg(Color::White),
        ),
    ]));
    line_idx += 1;

    // Session live for (Xh Ym Zs)
    let hours = session_secs / 3600;
    let minutes = (session_secs % 3600) / 60;
    let seconds = session_secs % 60;
    let sess_fmt = format!("{}h {}m {}s", hours, minutes, seconds);
    stats_lines.push(Line::from(vec![
        Span::styled("Session live for: ", Style::default().fg(Color::Gray)),
        Span::styled(sess_fmt, Style::default().fg(Color::White)),
    ]));
    line_idx += 1;

    stats_lines.push(Line::from(""));
    line_idx += 1;

    stats_lines.push(Line::from(vec![
        Span::styled("Open Positions: ", Style::default().fg(Color::Gray)),
        Span::styled(
            format!("{}", stats.open_positions),
            Style::default().fg(Color::Cyan),
        ),
    ]));
    line_idx += 1;

    stats_lines.push(Line::from(vec![
        Span::styled("Unrealized PnL: ", Style::default().fg(Color::Gray)),
        Span::styled(
            format!("{:+.6} WBNB", stats.total_pnl_open),
            Style::default().fg(if stats.total_pnl_open >= 0.0 {
                Color::Green
            } else {
                Color::Red
            }),
        ),
    ]));
    line_idx += 1;

    // Prepare indices for Take/Freeze buttons
    let mut take_button_lines: Vec<(usize, String, bool, bool)> = Vec::new();
    let mut take_all_line: Option<usize> = None;

    if !open_pos.is_empty() {
        stats_lines.push(Line::from(""));
        line_idx += 1;
        stats_lines.push(Line::from(Span::styled(
            "═══ Open Positions ═══",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        line_idx += 1;
        // Blank line below header where we place the Take All overlay
        take_all_line = Some(line_idx);
        stats_lines.push(Line::from(""));
        line_idx += 1;

        for pos in open_pos.iter() {
            let dex_label = match pos.dex_type {
                DexType::V2 => "v2",
                DexType::V3 => "v3",
                DexType::FourMeme => "fm",
            };

            // First line for this position (we overlay Take here)
            let this_first_line = line_idx;
            let mut sym_style = if pos.frozen {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::White)
            };
            if pos.out_of_liq {
                sym_style = Style::default().fg(Color::Red);
            }
            let mut spans = vec![
                Span::styled(
                    format!("[{}] ", dex_label),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(format!("{} ", pos.base_token), sym_style),
                Span::styled(
                    format!("{:+.2}%", pos.pnl_pct),
                    Style::default().fg(if pos.pnl_pct >= 0.0 {
                        Color::Green
                    } else {
                        Color::Red
                    }),
                ),
                Span::styled(
                    format!(" ({:+.6} WBNB)", pos.total_pnl_wbnb()),
                    Style::default().fg(Color::Gray),
                ),
            ];
            if pos.out_of_liq {
                spans.push(Span::styled(
                    " (out of LQ)",
                    Style::default().fg(Color::Red),
                ));
            }
            stats_lines.push(Line::from(spans));
            line_idx += 1;

            let dur = pos.duration_secs();
            stats_lines.push(Line::from(vec![Span::styled(
                format!(
                    "  Entry: {:.13} | Now: {:.13} | {}s ago",
                    pos.entry_price, pos.current_price, dur
                ),
                Style::default().fg(Color::DarkGray),
            )]));
            line_idx += 1;

            take_button_lines.push((
                this_first_line,
                pos.pair_address.clone(),
                pos.frozen,
                pos.needs_liq_ack,
            ));
        }
    }

    // Apply scroll with scrollbar
    let content_len = stats_lines.len();
    let viewport_len = area.height as usize;
    let max_scroll = content_len.saturating_sub(viewport_len);
    if results_scroll > max_scroll {
        results_scroll = max_scroll;
    }

    let p = ratatui::widgets::Paragraph::new(stats_lines).scroll((results_scroll as u16, 0));
    f.render_widget(p, area);

    // Render scrollbar if needed
    if content_len > viewport_len {
        use ratatui::widgets::{Scrollbar, ScrollbarOrientation};
        *results_scroll_state = results_scroll_state
            .content_length(content_len)
            .viewport_content_length(viewport_len)
            .position(results_scroll);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("↑"))
                .end_symbol(Some("↓")),
            area,
            results_scroll_state,
        );
    }

    // Overlay clickable Take/Freeze/Take All buttons (gray, underlined)
    results.take_btns.clear();
    results.freeze_btns.clear();
    results.remove_btns.clear();
    results.partial_btns.clear();
    results.take_all_btn = None;
    let take_style = Style::default()
        .fg(Color::Gray)
        .add_modifier(Modifier::UNDERLINED);

    let pending_liq_ack = sim_engine.has_pending_liq_alert();

    // Place Take All
    if let Some(line) = take_all_line {
        if line >= results_scroll && line < results_scroll.saturating_add(viewport_len) {
            let y = area.y + (line - results_scroll) as u16;
            let label = if pending_liq_ack { "Ok" } else { "Take All" };
            let w = label.len() as u16;
            let x = area.x + area.width.saturating_sub(w + 2);
            let r = Rect {
                x,
                y,
                width: w,
                height: 1,
            };
            let para =
                ratatui::widgets::Paragraph::new(Line::from(Span::styled(label, take_style)));
            f.render_widget(para, r);
            results.take_all_btn = Some(r);
        }
    }

    // Place Freeze and Take per position
    for (line, pair_addr, frozen_now, needs_ack) in take_button_lines.iter() {
        let li = *line;
        if li >= results_scroll && li < results_scroll.saturating_add(viewport_len) {
            let y = area.y + (li - results_scroll) as u16;
            let take_label = if *needs_ack { "Ok" } else { "Take" };
            let freeze_label = if *needs_ack {
                "Ok"
            } else if *frozen_now {
                "Unfreeze"
            } else {
                "Freeze"
            };
            let w_take = take_label.len() as u16;
            let w_freeze = freeze_label.len() as u16;
            let remove_label = "Remove";
            let w_remove = remove_label.len() as u16;
            // Right-align: [Remove][space][Freeze][space][Take]
            let x_take = area.x + area.width.saturating_sub(w_take + 2);
            let x_freeze = x_take.saturating_sub(w_freeze + 1);
            let x_remove = x_freeze.saturating_sub(w_remove + 1);
            let r_take = Rect {
                x: x_take,
                y,
                width: w_take,
                height: 1,
            };
            let r_freeze = Rect {
                x: x_freeze,
                y,
                width: w_freeze,
                height: 1,
            };
            let r_remove = Rect {
                x: x_remove,
                y,
                width: w_remove,
                height: 1,
            };
            let para_take =
                ratatui::widgets::Paragraph::new(Line::from(Span::styled(take_label, take_style)));
            let para_freeze = ratatui::widgets::Paragraph::new(Line::from(Span::styled(
                freeze_label,
                take_style,
            )));
            let para_remove = ratatui::widgets::Paragraph::new(Line::from(Span::styled(
                remove_label,
                take_style,
            )));
            f.render_widget(para_remove, r_remove);
            f.render_widget(para_freeze, r_freeze);
            f.render_widget(para_take, r_take);
            results.freeze_btns.push((r_freeze, pair_addr.clone()));
            results.take_btns.push((r_take, pair_addr.clone()));
            results.remove_btns.push((r_remove, pair_addr.clone()));
        }
    }

    // Place partial sell buttons (10% 25% 50%) on the line below each position's first line
    for (line, pair_addr, _frozen_now, needs_ack) in take_button_lines.iter() {
        let li = *line + 1;
        if li >= results_scroll && li < results_scroll.saturating_add(viewport_len) {
            let y = area.y + (li - results_scroll) as u16;
            let lbl_10 = if *needs_ack { "Ok" } else { "10%" };
            let lbl_25 = if *needs_ack { "Ok" } else { "25%" };
            let lbl_50 = if *needs_ack { "Ok" } else { "50%" };
            let w10 = lbl_10.len() as u16;
            let w25 = lbl_25.len() as u16;
            let w50 = lbl_50.len() as u16;
            let x50 = area.x + area.width.saturating_sub(w50 + 2);
            let x25 = x50.saturating_sub(w25 + 1);
            let x10 = x25.saturating_sub(w10 + 1);
            let r10 = Rect {
                x: x10,
                y,
                width: w10,
                height: 1,
            };
            let r25 = Rect {
                x: x25,
                y,
                width: w25,
                height: 1,
            };
            let r50 = Rect {
                x: x50,
                y,
                width: w50,
                height: 1,
            };
            let para10 =
                ratatui::widgets::Paragraph::new(Line::from(Span::styled(lbl_10, take_style)));
            let para25 =
                ratatui::widgets::Paragraph::new(Line::from(Span::styled(lbl_25, take_style)));
            let para50 =
                ratatui::widgets::Paragraph::new(Line::from(Span::styled(lbl_50, take_style)));
            f.render_widget(para10, r10);
            f.render_widget(para25, r25);
            f.render_widget(para50, r50);
            results.partial_btns.push((r10, pair_addr.clone(), 0.10));
            results.partial_btns.push((r25, pair_addr.clone(), 0.25));
            results.partial_btns.push((r50, pair_addr.clone(), 0.50));
        }
    }
}

pub fn results_interactions(
    results_areas: &ResultsAreas,
    sim_engine: &mut SimEngine,
    sold_pairs: &mut HashSet<String>,
    mx: u16,
    my: u16,
) {
    let contains_rect =
        |r: Rect| -> bool { mx >= r.x && mx < r.x + r.width && my >= r.y && my < r.y + r.height };
    let pending_liq_ack = sim_engine.has_pending_liq_alert();
    // Take All
    if let Some(r) = results_areas.take_all_btn {
        if contains_rect(r) {
            if pending_liq_ack {
                let cleared = sim_engine.ack_all_liq_alerts();
                if cleared > 0 {
                    save_log_to_file(&format!(
                        "[sim] acknowledged {} low-liquidity alert(s)",
                        cleared
                    ));
                }
            } else {
                let closed = sim_engine.take_all();
                if !closed.is_empty() {
                    for pos in closed.into_iter() {
                        sold_pairs.insert(pos.pair_address.clone());
                        save_log_to_file(&format!(
                            "[sim] ✓ MANUAL TAKE ALL closed {} ({}) PnL: {:+.6} WBNB",
                            pos.base_token, pos.pair_address, pos.pnl_wbnb
                        ));
                    }
                }
            }
        }
    }
    // Partial sell buttons
    for (r, pair_addr, fraction) in results_areas.partial_btns.iter() {
        if contains_rect(*r) {
            if sim_engine.position_needs_liq_ack(pair_addr) {
                break;
            }
            if let Some((realized, closed_now)) = sim_engine.partial_take(pair_addr, *fraction) {
                let pct = (*fraction * 100.0).round() as i32;
                if closed_now {
                    save_log_to_file(&format!(
                        "[sim] ◐ PARTIAL TAKE {}% ({}) realized: {:+.6} WBNB (remaining 0%, click Take to close)",
                        pct, pair_addr, realized
                    ));
                } else {
                    save_log_to_file(&format!(
                        "[sim] ◐ PARTIAL TAKE {}% ({}) realized: {:+.6} WBNB",
                        pct, pair_addr, realized
                    ));
                }
            }
            break;
        }
    }
    // Individual Freeze buttons
    for (r, pair_addr) in results_areas.freeze_btns.iter() {
        if contains_rect(*r) {
            if sim_engine.position_needs_liq_ack(pair_addr) {
                break;
            }
            if let Some(new_state) = sim_engine.toggle_freeze(pair_addr) {
                if new_state {
                    save_log_to_file(&format!("[sim] ◼ FROZEN {} ({})", pair_addr, pair_addr));
                } else {
                    save_log_to_file(&format!("[sim] ◻ UNFROZEN {} ({})", pair_addr, pair_addr));
                }
            }
            break;
        }
    }
    // Individual Take buttons
    for (r, pair_addr) in results_areas.take_btns.iter() {
        if contains_rect(*r) {
            if sim_engine.position_needs_liq_ack(pair_addr) {
                break;
            }
            if let Some(pos) = sim_engine.take_position(pair_addr) {
                sold_pairs.insert(pos.pair_address.clone());
                save_log_to_file(&format!(
                    "[sim] ✓ MANUAL TAKE closed {} ({}) PnL: {:+.6} WBNB",
                    pos.base_token, pos.pair_address, pos.pnl_wbnb
                ));
            }
            break;
        }
    }
}
