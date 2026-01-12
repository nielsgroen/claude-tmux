use ansi_to_tui::IntoText;
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Mode, NewSessionField};
use crate::session::ClaudeCodeStatus;

/// Render the application UI
pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Calculate preview height (roughly 50% of available space, min 8, max 20 lines)
    let available_height = area.height.saturating_sub(4); // minus header, status, footer
    let preview_height = (available_height * 50 / 100).clamp(8, 20);

    // Main layout: header, session list, preview, status bar, footer
    let layout = Layout::vertical([
        Constraint::Length(1),              // Header
        Constraint::Min(3),                 // Session list
        Constraint::Length(preview_height), // Preview pane
        Constraint::Length(1),              // Status bar
        Constraint::Length(1),              // Footer
    ])
    .split(area);

    render_header(frame, app, layout[0]);
    render_session_list(frame, app, layout[1]);
    render_preview(frame, app, layout[2]);
    render_status_bar(frame, app, layout[3]);
    render_footer(frame, app, layout[4]);

    // Render modal overlays
    match &app.mode {
        Mode::ConfirmKill { session_name } => {
            render_confirm_kill(frame, session_name);
        }
        Mode::NewSession { name, path, field } => {
            render_new_session_dialog(frame, name, path, *field);
        }
        Mode::Rename { old_name, new_name } => {
            render_rename_dialog(frame, old_name, new_name);
        }
        Mode::Filter { input } => {
            render_filter_bar(frame, input, layout[3]);
        }
        Mode::Help => {
            render_help(frame);
        }
        Mode::Normal => {}
    }

    // Render error/message overlay
    if let Some(ref error) = app.error {
        render_message(frame, error, Color::Red);
    } else if let Some(ref message) = app.message {
        render_message(frame, message, Color::Green);
    }
}

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let current = app
        .current_session
        .as_ref()
        .map(|s| format!(" attached: {} ", s))
        .unwrap_or_default();

    let title = format!("─ CCM ─{:─>width$}", current, width = area.width as usize - 7);

    let header = Paragraph::new(title)
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));

    frame.render_widget(header, area);
}

fn render_session_list(frame: &mut Frame, app: &App, area: Rect) {
    let filtered = app.filtered_sessions();

    if filtered.is_empty() {
        let empty_msg = if app.filter.is_empty() {
            "No tmux sessions found. Press 'n' to create one."
        } else {
            "No sessions match the filter."
        };
        let paragraph = Paragraph::new(empty_msg)
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        frame.render_widget(paragraph, area);
        return;
    }

    // Calculate column widths
    let max_name_len = filtered
        .iter()
        .map(|s| s.name.width())
        .max()
        .unwrap_or(10)
        .max(10);

    let items: Vec<ListItem> = filtered
        .iter()
        .enumerate()
        .map(|(i, session)| {
            let is_selected = i == app.selected;
            let is_current = app
                .current_session
                .as_ref()
                .is_some_and(|c| c == &session.name);

            // Build the line
            let marker = if is_selected { "▸" } else { " " };
            let status = &session.claude_code_status;

            // Use brighter colors when selected so text is readable on dark background
            let status_color = match (status, is_selected) {
                (ClaudeCodeStatus::Working, _) => Color::Green,
                (ClaudeCodeStatus::WaitingInput, _) => Color::Yellow,
                (ClaudeCodeStatus::Idle, true) => Color::White,
                (ClaudeCodeStatus::Idle, false) => Color::DarkGray,
                (ClaudeCodeStatus::Unknown, true) => Color::Gray,
                (ClaudeCodeStatus::Unknown, false) => Color::DarkGray,
            };

            let path_color = if is_selected { Color::White } else { Color::DarkGray };

            let name_style = if is_current {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let line = Line::from(vec![
                Span::raw(format!(" {} ", marker)),
                Span::styled(
                    format!("{:<width$}", session.name, width = max_name_len),
                    name_style,
                ),
                Span::raw("  "),
                Span::styled(status.symbol(), Style::default().fg(status_color)),
                Span::raw(" "),
                Span::styled(
                    format!("{:<8}", status.label()),
                    Style::default().fg(status_color),
                ),
                Span::raw("  "),
                Span::styled(session.display_path(), Style::default().fg(path_color)),
            ]);

            let style = if is_selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            ListItem::new(line).style(style)
        })
        .collect();

    let list = List::new(items);
    frame.render_widget(list, area);
}

fn render_preview(frame: &mut Frame, app: &App, area: Rect) {
    // Clear the entire preview area first to prevent stale content
    frame.render_widget(Clear, area);

    // Draw separator lines at top and bottom
    let separator = "─".repeat(area.width as usize);

    let top_sep_area = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: 1,
    };
    let top_sep = Paragraph::new(separator.clone()).style(Style::default().fg(Color::DarkGray));
    frame.render_widget(top_sep, top_sep_area);

    let bottom_sep_area = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(1),
        width: area.width,
        height: 1,
    };
    let bottom_sep = Paragraph::new(separator).style(Style::default().fg(Color::White));
    frame.render_widget(bottom_sep, bottom_sep_area);

    // Content area (between separators)
    let content_area = Rect {
        x: area.x,
        y: area.y + 1,
        width: area.width,
        height: area.height.saturating_sub(2),
    };

    let content = match &app.preview_content {
        Some(text) if !text.is_empty() => text,
        _ => {
            let msg = Paragraph::new("  No preview available")
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(msg, content_area);
            return;
        }
    };

    // Parse ANSI escape sequences into styled ratatui Text
    let styled_text = match content.into_text() {
        Ok(text) => text,
        Err(_) => {
            // Fallback to plain text if parsing fails
            Text::raw(content)
        }
    };

    // Take only the last N lines that fit in the content area
    let available_lines = content_area.height as usize;
    let total_lines = styled_text.lines.len();
    let start = total_lines.saturating_sub(available_lines);
    let visible_lines: Vec<Line> = styled_text.lines.into_iter().skip(start).collect();

    let preview = Paragraph::new(visible_lines);
    frame.render_widget(preview, content_area);
}

fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let (working, waiting, _idle) = app.status_counts();
    let total = app.sessions.len();

    let mut parts = vec![format!("{} sessions", total)];

    if working > 0 {
        parts.push(format!("{} working", working));
    }
    if waiting > 0 {
        parts.push(format!("{} awaiting input", waiting));
    }

    let status = parts.join(" │ ");

    let filter_info = if !app.filter.is_empty() {
        format!(" │ filter: \"{}\"", app.filter)
    } else {
        String::new()
    };

    let text = format!("  {}{}", status, filter_info);

    let bar = Paragraph::new(text).style(Style::default().fg(Color::DarkGray));

    frame.render_widget(bar, area);
}

fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let hints = match app.mode {
        Mode::Normal => "  ? help  ↑↓/jk navigate  ⏎ switch  n new  K kill  r rename  / filter  R refresh  q quit",
        Mode::Filter { .. } => "  ⏎ apply  esc cancel",
        Mode::ConfirmKill { .. } => "  ⏎/y confirm  n/esc cancel",
        Mode::NewSession { .. } => "  ⏎ create  tab switch field  esc cancel",
        Mode::Rename { .. } => "  ⏎ confirm  esc cancel",
        Mode::Help => "  q close",
    };

    let footer = Paragraph::new(hints).style(Style::default().fg(Color::DarkGray));

    frame.render_widget(footer, area);
}

fn render_filter_bar(frame: &mut Frame, input: &str, area: Rect) {
    frame.render_widget(Clear, area);
    let text = format!("  / {}", input);
    let bar = Paragraph::new(text).style(Style::default().fg(Color::Yellow));
    frame.render_widget(bar, area);
}

fn render_confirm_kill(frame: &mut Frame, session_name: &str) {
    let area = centered_rect(50, 5, frame.area());

    let block = Block::default()
        .title(" Kill Session ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red));

    let text = format!("Kill session '{}'?\n\n[Y]es  [n]o", session_name);
    let paragraph = Paragraph::new(text)
        .block(block)
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

fn render_new_session_dialog(frame: &mut Frame, name: &str, path: &str, field: NewSessionField) {
    let area = centered_rect(60, 8, frame.area());

    let block = Block::default()
        .title(" New Session ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let name_style = if field == NewSessionField::Name {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    let path_style = if field == NewSessionField::Path {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    let text = Text::from(vec![
        Line::from(vec![
            Span::styled("Name: ", name_style),
            Span::raw(name),
            if field == NewSessionField::Name {
                Span::raw("_")
            } else {
                Span::raw("")
            },
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Path: ", path_style),
            Span::raw(path),
            if field == NewSessionField::Path {
                Span::raw("_")
            } else {
                Span::raw("")
            },
        ]),
        Line::raw(""),
        Line::styled(
            "Press Enter to create, Tab to switch fields",
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    let paragraph = Paragraph::new(text)
        .block(block)
        .wrap(Wrap { trim: true });

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

fn render_rename_dialog(frame: &mut Frame, old_name: &str, new_name: &str) {
    let area = centered_rect(50, 6, frame.area());

    let block = Block::default()
        .title(format!(" Rename '{}' ", old_name))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let text = Text::from(vec![
        Line::from(vec![
            Span::raw("New name: "),
            Span::styled(new_name, Style::default().fg(Color::Yellow)),
            Span::raw("_"),
        ]),
        Line::raw(""),
        Line::styled(
            "Press Enter to confirm",
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    let paragraph = Paragraph::new(text)
        .block(block)
        .wrap(Wrap { trim: true });

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

fn render_help(frame: &mut Frame) {
    let area = centered_rect(60, 16, frame.area());

    let block = Block::default()
        .title(" Help ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let help_text = vec![
        Line::from(Span::styled(
            "Navigation",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::raw("  j / ↓       Move down"),
        Line::raw("  k / ↑       Move up"),
        Line::raw("  Enter       Switch to session"),
        Line::raw(""),
        Line::from(Span::styled(
            "Actions",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::raw("  n           New session"),
        Line::raw("  K           Kill session"),
        Line::raw("  r           Rename session"),
        Line::raw("  /           Filter sessions"),
        Line::raw("  R           Refresh list"),
        Line::raw(""),
        Line::from(Span::styled(
            "Other",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::raw("  ?           Show this help"),
        Line::raw("  q / Esc     Quit"),
    ];

    let paragraph = Paragraph::new(help_text)
        .block(block)
        .wrap(Wrap { trim: true });

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

fn render_message(frame: &mut Frame, message: &str, color: Color) {
    let area = frame.area();
    let msg_area = Rect {
        x: 2,
        y: area.height.saturating_sub(3),
        width: area.width.saturating_sub(4),
        height: 1,
    };

    let text = format!(" {} ", message);
    let paragraph = Paragraph::new(text)
        .style(Style::default().fg(Color::White).bg(color));

    frame.render_widget(paragraph, msg_area);
}

/// Create a centered rectangle of the given size within the parent area
fn centered_rect(width: u16, height: u16, parent: Rect) -> Rect {
    let x = parent.x + (parent.width.saturating_sub(width)) / 2;
    let y = parent.y + (parent.height.saturating_sub(height)) / 2;

    Rect {
        x,
        y,
        width: width.min(parent.width),
        height: height.min(parent.height),
    }
}
