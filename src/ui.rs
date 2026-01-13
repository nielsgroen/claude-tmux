use ansi_to_tui::IntoText;
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, CreatePullRequestField, Mode, NewSessionField, NewWorktreeField};
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
        Mode::ConfirmAction => {
            render_confirm_action(frame, app);
        }
        Mode::NewSession { name, path, field } => {
            render_new_session_dialog(frame, name, path, *field);
        }
        Mode::Rename { old_name, new_name } => {
            render_rename_dialog(frame, old_name, new_name);
        }
        Mode::Commit { message } => {
            render_commit_dialog(frame, message);
        }
        Mode::NewWorktree {
            branch_input,
            selected_branch,
            worktree_path,
            session_name,
            field,
            ..
        } => {
            render_new_worktree_dialog(
                frame,
                app,
                branch_input,
                *selected_branch,
                worktree_path,
                session_name,
                *field,
            );
        }
        Mode::Filter { input } => {
            render_filter_bar(frame, input, layout[3]);
        }
        Mode::CreatePullRequest {
            title,
            body,
            base_branch,
            field,
        } => {
            render_create_pr_dialog(frame, title, body, base_branch, *field);
        }
        Mode::Help => {
            render_help(frame);
        }
        Mode::Normal | Mode::ActionMenu => {}
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

    let title = format!("─ claude-tmux ─{:─>width$}", current, width = area.width as usize - 15);

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

    let mut items: Vec<ListItem> = Vec::new();

    for (i, session) in filtered.iter().enumerate() {
        let is_selected = i == app.selected;
        let is_current = app
            .current_session
            .as_ref()
            .is_some_and(|c| c == &session.name);

        // Show ▾ when action menu is open for this session, ▸ when selected but collapsed
        let is_expanded = is_selected && matches!(app.mode, Mode::ActionMenu);
        let marker = if is_selected {
            if is_expanded { "▾" } else { "▸" }
        } else {
            " "
        };
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

        // Build git info spans
        let git_spans = if let Some(ref git) = session.git_context {
            let (open, close) = if git.is_worktree {
                ("[", "]")
            } else {
                ("(", ")")
            };
            let bracket_color = if git.is_worktree {
                Color::Magenta
            } else {
                Color::Cyan
            };

            // Show status indicators: + for staged, * for unstaged, ? for untracked
            let mut status_str = String::new();
            if git.has_staged {
                status_str.push('+');
            }
            if git.has_unstaged {
                status_str.push('*');
            }
            // if git.has_untracked {
            //     status_str.push('?');
            // }
            let status_spans = if !status_str.is_empty() {
                let color = if git.has_staged && !git.has_unstaged {
                    Color::Green // Only staged = green
                } else {
                    Color::Yellow // Mixed state = yellow
                };
                vec![Span::styled(format!(" {}", status_str), Style::default().fg(color))]
            } else {
                vec![]
            };

            let mut spans = vec![
                Span::raw(" "),
                Span::styled(open, Style::default().fg(bracket_color)),
                Span::styled(&git.branch, Style::default().fg(Color::Cyan)),
                Span::styled(close, Style::default().fg(bracket_color)),
            ];
            spans.extend(status_spans);
            spans
        } else {
            vec![]
        };

        let mut line_spans = vec![
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
        ];
        line_spans.extend(git_spans);

        let line = Line::from(line_spans);

        let style = if is_selected {
            Style::default().bg(Color::DarkGray)
        } else {
            Style::default()
        };

        items.push(ListItem::new(line).style(style));

        // Show expanded content when in action menu mode for this session
        if is_expanded {
            let label_style = Style::default().fg(Color::DarkGray);
            let value_style = Style::default().fg(Color::White);

            // Session metadata row
            let attached_str = if session.attached { "yes" } else { "no" };
            let pane_count = session.panes.len();

            let meta_line = Line::from(vec![
                Span::raw("     "),
                Span::styled("windows: ", label_style),
                Span::styled(format!("{}", session.window_count), value_style),
                Span::raw("  "),
                Span::styled("panes: ", label_style),
                Span::styled(format!("{}", pane_count), value_style),
                Span::raw("  "),
                Span::styled("uptime: ", label_style),
                Span::styled(session.duration(), value_style),
                Span::raw("  "),
                Span::styled("attached: ", label_style),
                Span::styled(attached_str, value_style),
            ]);
            items.push(ListItem::new(meta_line));

            // Git metadata row (if available)
            if let Some(ref git) = session.git_context {
                let mut git_spans = vec![
                    Span::raw("     "),
                    Span::styled("branch: ", label_style),
                    Span::styled(&git.branch, Style::default().fg(Color::Cyan)),
                ];

                if git.ahead > 0 || git.behind > 0 {
                    git_spans.push(Span::raw("  "));
                    if git.ahead > 0 {
                        git_spans.push(Span::styled(
                            format!("↑{}", git.ahead),
                            Style::default().fg(Color::Green),
                        ));
                    }
                    if git.behind > 0 {
                        if git.ahead > 0 {
                            git_spans.push(Span::raw(" "));
                        }
                        git_spans.push(Span::styled(
                            format!("↓{}", git.behind),
                            Style::default().fg(Color::Red),
                        ));
                    }
                }

                // Show staged/unstaged status
                if git.has_staged {
                    git_spans.push(Span::raw("  "));
                    git_spans.push(Span::styled("staged: ", label_style));
                    git_spans.push(Span::styled("yes", Style::default().fg(Color::Green)));
                }

                if git.has_unstaged {
                    git_spans.push(Span::raw("  "));
                    git_spans.push(Span::styled("unstaged: ", label_style));
                    git_spans.push(Span::styled("yes", Style::default().fg(Color::Yellow)));
                }

                if git.is_worktree {
                    git_spans.push(Span::raw("  "));
                    git_spans.push(Span::styled("worktree: ", label_style));
                    git_spans.push(Span::styled("yes", Style::default().fg(Color::Magenta)));
                }

                items.push(ListItem::new(Line::from(git_spans)));
            }

            // Separator
            let sep_line = Line::from(Span::styled(
                "     ────────────────────────",
                Style::default().fg(Color::DarkGray),
            ));
            items.push(ListItem::new(sep_line));

            // Action items
            for (action_idx, action) in app.available_actions.iter().enumerate() {
                let is_action_selected = action_idx == app.selected_action;
                let action_marker = if is_action_selected { "▸" } else { " " };
                let action_style = if is_action_selected {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default().fg(Color::White)
                };

                let action_line = Line::from(vec![
                    Span::raw("     "),
                    Span::styled(format!("{} {}", action_marker, action.label()), action_style),
                ]);
                items.push(ListItem::new(action_line));
            }

            // White separator at end of submenu
            let end_sep = Line::from(Span::styled(
                "",
                Style::default().fg(Color::White),
            ));
            items.push(ListItem::new(end_sep));
        }
    }

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
        Mode::Normal => "  ? help  jk navigate  l actions  ⏎ switch  n new  K kill  r rename  / filter  q quit",
        Mode::ActionMenu => "  jk navigate  ⏎/l select  h/esc back  q quit",
        Mode::Filter { .. } => "  ⏎ apply  esc cancel",
        Mode::ConfirmAction => "  y/⏎ confirm  n/esc cancel",
        Mode::NewSession { .. } => "  ⏎ create  tab switch field  esc cancel",
        Mode::Rename { .. } => "  ⏎ confirm  esc cancel",
        Mode::Commit { .. } => "  ⏎ commit  esc cancel",
        Mode::NewWorktree { .. } => "  ⏎ create  tab switch field  ↑↓ select branch  esc cancel",
        Mode::CreatePullRequest { .. } => "  ⏎ create PR  tab switch field  esc cancel",
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

fn render_confirm_action(frame: &mut Frame, app: &App) {
    use crate::app::SessionAction;

    let session = app.selected_session();
    let session_name = session.map(|s| s.name.as_str()).unwrap_or("?");

    let action_desc = app
        .pending_action
        .as_ref()
        .map(|a| a.label())
        .unwrap_or("Perform action");

    // Check if this is a worktree deletion to show enhanced dialog
    let is_worktree_delete = matches!(app.pending_action, Some(SessionAction::KillAndDeleteWorktree));

    if is_worktree_delete {
        let worktree_path = session
            .map(|s| s.display_path())
            .unwrap_or_else(|| "?".to_string());

        let area = centered_rect(55, 9, frame.area());

        let block = Block::default()
            .title(" Confirm ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red));

        let text = Text::from(vec![
            Line::from(format!("Kill session '{}'", session_name)),
            Line::from("AND delete worktree at:"),
            Line::styled(format!("  {}", worktree_path), Style::default().fg(Color::Yellow)),
            Line::raw(""),
            Line::styled(
                "⚠ This will permanently delete the directory!",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Line::raw(""),
            Line::from("[Y]es  [n]o"),
        ]);

        let paragraph = Paragraph::new(text)
            .block(block)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });

        frame.render_widget(Clear, area);
        frame.render_widget(paragraph, area);
    } else {
        let area = centered_rect(50, 5, frame.area());

        let block = Block::default()
            .title(" Confirm ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red));

        let text = format!("{} '{}'?\n\n[Y]es  [n]o", action_desc, session_name);
        let paragraph = Paragraph::new(text)
            .block(block)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });

        frame.render_widget(Clear, area);
        frame.render_widget(paragraph, area);
    }
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

fn render_commit_dialog(frame: &mut Frame, message: &str) {
    let area = centered_rect(60, 6, frame.area());

    let block = Block::default()
        .title(" Commit ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let text = Text::from(vec![
        Line::from(vec![
            Span::raw("Message: "),
            Span::styled(message, Style::default().fg(Color::Yellow)),
            Span::raw("_"),
        ]),
        Line::raw(""),
        Line::styled(
            "Press Enter to commit",
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    let paragraph = Paragraph::new(text)
        .block(block)
        .wrap(Wrap { trim: true });

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

fn render_create_pr_dialog(
    frame: &mut Frame,
    title: &str,
    body: &str,
    base_branch: &str,
    field: CreatePullRequestField,
) {
    let area = centered_rect(65, 12, frame.area());

    let block = Block::default()
        .title(" Create Pull Request ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green));

    let title_style = if field == CreatePullRequestField::Title {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    let body_style = if field == CreatePullRequestField::Body {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    let base_style = if field == CreatePullRequestField::BaseBranch {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    let cursor = |active: bool| if active { "_" } else { "" };

    let text = Text::from(vec![
        Line::from(vec![
            Span::styled("Title: ", title_style),
            Span::styled(title, Style::default().fg(Color::Yellow)),
            Span::raw(cursor(field == CreatePullRequestField::Title)),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Body:  ", body_style),
            Span::styled(
                if body.is_empty() { "(optional)" } else { body },
                if body.is_empty() {
                    Style::default().fg(Color::DarkGray)
                } else {
                    Style::default().fg(Color::Yellow)
                },
            ),
            Span::raw(cursor(field == CreatePullRequestField::Body)),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Base:  ", base_style),
            Span::styled(base_branch, Style::default().fg(Color::Cyan)),
            Span::raw(cursor(field == CreatePullRequestField::BaseBranch)),
        ]),
        Line::raw(""),
        Line::styled(
            "[Tab] Next field  [Enter] Create PR  [Esc] Cancel",
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    let paragraph = Paragraph::new(text)
        .block(block)
        .wrap(Wrap { trim: false });

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

fn render_new_worktree_dialog(
    frame: &mut Frame,
    app: &App,
    branch_input: &str,
    selected_branch: Option<usize>,
    worktree_path: &str,
    session_name: &str,
    field: NewWorktreeField,
) {
    // Get filtered branches
    let filtered_branches = app.filtered_branches();
    let is_new_branch = selected_branch.is_none()
        && !branch_input.is_empty()
        && !filtered_branches.contains(&branch_input);

    // Calculate dialog height based on branches shown (max 5)
    let branches_to_show = filtered_branches.len().min(5);
    let dialog_height = 10 + branches_to_show as u16;

    let area = centered_rect(65, dialog_height, frame.area());

    let block = Block::default()
        .title(" New Session from Worktree ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    // Build the content
    let mut lines = Vec::new();

    // Branch field
    let branch_style = if field == NewWorktreeField::Branch {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    let branch_indicator = if is_new_branch {
        Span::styled(" (new)", Style::default().fg(Color::Green))
    } else if selected_branch.is_some() {
        Span::styled(" (existing)", Style::default().fg(Color::Cyan))
    } else {
        Span::raw("")
    };

    lines.push(Line::from(vec![
        Span::styled("Branch:  ", branch_style),
        Span::styled(branch_input, Style::default().fg(Color::Yellow)),
        if field == NewWorktreeField::Branch {
            Span::raw("_")
        } else {
            Span::raw("")
        },
        branch_indicator,
    ]));

    // Show filtered branches if in branch field
    if field == NewWorktreeField::Branch && !filtered_branches.is_empty() {
        lines.push(Line::styled(
            "         ─────────────────────────────",
            Style::default().fg(Color::DarkGray),
        ));

        for (i, branch) in filtered_branches.iter().take(5).enumerate() {
            let is_selected = selected_branch == Some(i);
            let prefix = if is_selected { "       > " } else { "         " };
            let style = if is_selected {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            lines.push(Line::styled(format!("{}{}", prefix, branch), style));
        }

        if filtered_branches.len() > 5 {
            lines.push(Line::styled(
                format!("         ... and {} more", filtered_branches.len() - 5),
                Style::default().fg(Color::DarkGray),
            ));
        }

        lines.push(Line::styled(
            "         ─────────────────────────────",
            Style::default().fg(Color::DarkGray),
        ));
    }

    lines.push(Line::raw(""));

    // Path field
    let path_style = if field == NewWorktreeField::Path {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    lines.push(Line::from(vec![
        Span::styled("Path:    ", path_style),
        Span::styled(worktree_path, Style::default().fg(Color::Yellow)),
        if field == NewWorktreeField::Path {
            Span::raw("_")
        } else {
            Span::raw("")
        },
    ]));

    lines.push(Line::raw(""));

    // Session name field
    let session_style = if field == NewWorktreeField::SessionName {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    lines.push(Line::from(vec![
        Span::styled("Session: ", session_style),
        Span::styled(session_name, Style::default().fg(Color::Yellow)),
        if field == NewWorktreeField::SessionName {
            Span::raw("_")
        } else {
            Span::raw("")
        },
    ]));

    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "[Tab] Next field  [Enter] Create  [Esc] Cancel",
        Style::default().fg(Color::DarkGray),
    ));

    let text = Text::from(lines);
    let paragraph = Paragraph::new(text)
        .block(block)
        .wrap(Wrap { trim: false });

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
    let area = centered_rect(60, 21, frame.area());

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
        Line::raw("  l / →       Open action menu"),
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
            "Action Menu",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::raw("  h / ←       Go back"),
        Line::raw("  Enter       Execute action"),
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

    // Calculate height needed (at least 1, up to 3 for longer messages)
    let max_width = area.width.saturating_sub(6) as usize;
    let lines_needed = if max_width > 0 {
        (message.len() / max_width + 1).min(3)
    } else {
        1
    };
    let height = lines_needed as u16;

    let msg_area = Rect {
        x: 2,
        y: area.height.saturating_sub(2 + height),
        width: area.width.saturating_sub(4),
        height,
    };

    let text = format!(" {} ", message);
    let paragraph = Paragraph::new(text)
        .style(Style::default().fg(Color::White).bg(color))
        .wrap(Wrap { trim: true });

    frame.render_widget(Clear, msg_area);
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
