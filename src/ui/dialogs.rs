//! Modal dialog rendering
//!
//! Provides rendering for all modal dialogs:
//! - Confirmation dialogs (kill, merge PR, etc.)
//! - Input dialogs (new session, rename, commit, new worktree, create PR)

use ratatui::{
    layout::Alignment,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, CreatePullRequestField, NewSessionField, NewWorktreeField, SessionAction};

use super::help::centered_rect;

pub fn render_confirm_action(frame: &mut Frame, app: &App) {
    let session = app.selected_session();
    let session_name = session.map(|s| s.name.as_str()).unwrap_or("?");
    let is_worktree = session
        .and_then(|s| s.git_context.as_ref())
        .map(|g| g.is_worktree)
        .unwrap_or(false);
    let is_current_session = app
        .current_session
        .as_ref()
        .is_some_and(|c| c == session_name);

    match &app.pending_action {
        Some(SessionAction::KillAndDeleteWorktree) => {
            let worktree_path = session
                .map(|s| s.display_path())
                .unwrap_or_else(|| "?".to_string());

            let dialog_height = if is_current_session { 11 } else { 9 };
            let area = centered_rect(55, dialog_height, frame.area());

            let block = Block::default()
                .title(" Confirm ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red));

            let mut lines = vec![
                Line::from(format!("Kill session '{}'", session_name)),
                Line::from("AND delete worktree at:"),
                Line::styled(
                    format!("  {}", worktree_path),
                    Style::default().fg(Color::Yellow),
                ),
                Line::raw(""),
                Line::styled(
                    "⚠ This will permanently delete the directory!",
                    Style::default()
                        .fg(Color::Red)
                        .add_modifier(Modifier::BOLD),
                ),
            ];

            if is_current_session {
                lines.push(Line::styled(
                    "⚠ This is your current session - tmux will exit!",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
            }

            lines.push(Line::raw(""));
            lines.push(Line::from("[Y]es  [n]o"));

            let paragraph = Paragraph::new(Text::from(lines))
                .block(block)
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true });

            frame.render_widget(Clear, area);
            frame.render_widget(paragraph, area);
        }
        Some(SessionAction::ClosePullRequest) => {
            let area = centered_rect(50, 5, frame.area());

            let block = Block::default()
                .title(" Close Pull Request ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow));

            let text = "Close this pull request without merging?\n\n[Y]es  [n]o";
            let paragraph = Paragraph::new(text)
                .block(block)
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true });

            frame.render_widget(Clear, area);
            frame.render_widget(paragraph, area);
        }
        Some(SessionAction::MergePullRequest) => {
            let area = centered_rect(50, 5, frame.area());

            let block = Block::default()
                .title(" Merge Pull Request ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green));

            let text = "Merge this pull request?\n\n[Y]es  [n]o";
            let paragraph = Paragraph::new(text)
                .block(block)
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true });

            frame.render_widget(Clear, area);
            frame.render_widget(paragraph, area);
        }
        Some(SessionAction::MergePullRequestAndClose) => {
            let dialog_height = if is_current_session { 12 } else { 10 };
            let area = centered_rect(58, dialog_height, frame.area());

            let block = Block::default()
                .title(" Merge PR + Close ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow));

            let mut lines = vec![
                Line::from("This will:"),
                Line::styled(
                    "  • Merge the pull request",
                    Style::default().fg(Color::Green),
                ),
            ];

            if is_worktree {
                lines.push(Line::styled(
                    "  • Remove the local worktree",
                    Style::default().fg(Color::Red),
                ));
            }

            lines.push(Line::styled(
                format!("  • Kill session '{}'", session_name),
                Style::default().fg(Color::Red),
            ));

            if is_current_session {
                lines.push(Line::raw(""));
                lines.push(Line::styled(
                    "⚠ This is your current session - tmux will exit!",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
            }

            lines.push(Line::raw(""));
            lines.push(Line::from("[Y]es  [n]o"));

            let paragraph = Paragraph::new(Text::from(lines))
                .block(block)
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true });

            frame.render_widget(Clear, area);
            frame.render_widget(paragraph, area);
        }
        Some(action) => {
            // Check if this action kills a session (currently only Kill action reaches here)
            let kills_session = matches!(action, SessionAction::Kill);
            let show_exit_warning = kills_session && is_current_session;

            let dialog_height = if show_exit_warning { 7 } else { 5 };
            let area = centered_rect(55, dialog_height, frame.area());

            let block = Block::default()
                .title(" Confirm ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red));

            let mut lines = vec![Line::from(format!(
                "{} '{}'?",
                action.label(),
                session_name
            ))];

            if show_exit_warning {
                lines.push(Line::raw(""));
                lines.push(Line::styled(
                    "⚠ This is your current session - tmux will exit!",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
            }

            lines.push(Line::raw(""));
            lines.push(Line::from("[Y]es  [n]o"));

            let paragraph = Paragraph::new(Text::from(lines))
                .block(block)
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true });

            frame.render_widget(Clear, area);
            frame.render_widget(paragraph, area);
        }
        None => {}
    }
}

pub fn render_new_session_dialog(
    frame: &mut Frame,
    name: &str,
    path: &str,
    field: NewSessionField,
    path_suggestions: &[String],
    path_selected: Option<usize>,
) {
    // Calculate dialog height based on suggestions shown
    let suggestions_to_show = if field == NewSessionField::Path && !path_suggestions.is_empty() {
        path_suggestions.len().min(5)
    } else {
        0
    };
    let suggestion_extra = if suggestions_to_show > 0 {
        2 + if path_suggestions.len() > 5 { 1 } else { 0 } // separators + optional "more"
    } else {
        0
    };
    let dialog_height = 8 + suggestions_to_show as u16 + suggestion_extra as u16;

    let area = centered_rect(60, dialog_height, frame.area());

    let block = Block::default()
        .title(" New Session ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let name_style = if field == NewSessionField::Name {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    let path_style = if field == NewSessionField::Path {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    let mut lines = Vec::new();

    // Name field
    lines.push(Line::from(vec![
        Span::styled("Name: ", name_style),
        Span::raw(name),
        if field == NewSessionField::Name {
            Span::raw("_")
        } else {
            Span::raw("")
        },
    ]));

    lines.push(Line::raw(""));

    // Path field with ghost text
    let ghost_text = if field == NewSessionField::Path {
        crate::completion::complete_path(path).ghost_text
    } else {
        None
    };

    let mut path_spans = vec![
        Span::styled("Path: ", path_style),
        Span::styled(path, Style::default().fg(Color::Yellow)),
    ];

    // Add ghost text (completion suffix)
    if let Some(ref ghost) = ghost_text {
        path_spans.push(Span::styled(
            ghost,
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        ));
    }

    // Add cursor
    if field == NewSessionField::Path {
        path_spans.push(Span::raw("_"));
    }

    lines.push(Line::from(path_spans));

    // Show path suggestions when path field is active
    if field == NewSessionField::Path && !path_suggestions.is_empty() {
        lines.push(Line::styled(
            "      ────────────────────────────────────",
            Style::default().fg(Color::DarkGray),
        ));

        for (i, suggestion) in path_suggestions.iter().take(5).enumerate() {
            let is_selected = path_selected == Some(i);
            let prefix = if is_selected { "    > " } else { "      " };
            let style = if is_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            lines.push(Line::styled(format!("{}{}", prefix, suggestion), style));
        }

        if path_suggestions.len() > 5 {
            lines.push(Line::styled(
                format!("      ... and {} more", path_suggestions.len() - 5),
                Style::default().fg(Color::DarkGray),
            ));
        }

        lines.push(Line::styled(
            "      ────────────────────────────────────",
            Style::default().fg(Color::DarkGray),
        ));
    }

    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "Tab switch  ↑↓ select  → accept  Enter create  Esc cancel",
        Style::default().fg(Color::DarkGray),
    ));

    let text = Text::from(lines);
    let paragraph = Paragraph::new(text)
        .block(block)
        .wrap(Wrap { trim: false });

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

pub fn render_commit_dialog(frame: &mut Frame, message: &str) {
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

pub fn render_create_pr_dialog(
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
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    let body_style = if field == CreatePullRequestField::Body {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    let base_style = if field == CreatePullRequestField::BaseBranch {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
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

#[allow(clippy::too_many_arguments)]
pub fn render_new_worktree_dialog(
    frame: &mut Frame,
    app: &App,
    branch_input: &str,
    selected_branch: Option<usize>,
    worktree_path: &str,
    session_name: &str,
    field: NewWorktreeField,
    path_suggestions: &[String],
    path_selected: Option<usize>,
) {
    // Get filtered branches
    let filtered_branches = app.filtered_branches();
    let is_new_branch = selected_branch.is_none()
        && !branch_input.is_empty()
        && !filtered_branches.contains(&branch_input);

    // Calculate dialog height based on suggestions shown
    let branches_to_show = if field == NewWorktreeField::Branch && !filtered_branches.is_empty() {
        filtered_branches.len().min(5)
    } else {
        0
    };
    let branch_extra = if branches_to_show > 0 {
        2 + if filtered_branches.len() > 5 { 1 } else { 0 }
    } else {
        0
    };
    let path_suggestions_to_show =
        if field == NewWorktreeField::Path && !path_suggestions.is_empty() {
            path_suggestions.len().min(5)
        } else {
            0
        };
    let path_extra = if path_suggestions_to_show > 0 {
        2 + if path_suggestions.len() > 5 { 1 } else { 0 }
    } else {
        0
    };
    let dialog_height = 10
        + branches_to_show as u16
        + branch_extra as u16
        + path_suggestions_to_show as u16
        + path_extra as u16;

    let area = centered_rect(65, dialog_height, frame.area());

    let block = Block::default()
        .title(" New Session from Worktree ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    // Build the content
    let mut lines = Vec::new();

    // Branch field with ghost text
    let branch_style = if field == NewWorktreeField::Branch {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
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

    // Calculate branch ghost text
    let branch_ghost = if field == NewWorktreeField::Branch {
        crate::completion::branch_ghost_text(branch_input, &filtered_branches, selected_branch)
    } else {
        None
    };

    let mut branch_spans = vec![
        Span::styled("Branch:  ", branch_style),
        Span::styled(branch_input, Style::default().fg(Color::Yellow)),
    ];

    // Add branch ghost text
    if let Some(ref ghost) = branch_ghost {
        branch_spans.push(Span::styled(
            ghost,
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        ));
    }

    // Add cursor
    if field == NewWorktreeField::Branch {
        branch_spans.push(Span::raw("_"));
    }

    branch_spans.push(branch_indicator);
    lines.push(Line::from(branch_spans));

    // Show filtered branches if in branch field
    if field == NewWorktreeField::Branch && !filtered_branches.is_empty() {
        lines.push(Line::styled(
            "         ─────────────────────────────",
            Style::default().fg(Color::DarkGray),
        ));

        for (i, branch) in filtered_branches.iter().take(5).enumerate() {
            let is_selected = selected_branch == Some(i);
            let prefix = if is_selected {
                "       > "
            } else {
                "         "
            };
            let style = if is_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
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

    // Path field with ghost text
    let path_style = if field == NewWorktreeField::Path {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    // Calculate path ghost text
    let path_ghost = if field == NewWorktreeField::Path {
        crate::completion::complete_path(worktree_path).ghost_text
    } else {
        None
    };

    let mut path_spans = vec![
        Span::styled("Path:    ", path_style),
        Span::styled(worktree_path, Style::default().fg(Color::Yellow)),
    ];

    // Add path ghost text
    if let Some(ref ghost) = path_ghost {
        path_spans.push(Span::styled(
            ghost,
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        ));
    }

    // Add cursor
    if field == NewWorktreeField::Path {
        path_spans.push(Span::raw("_"));
    }

    lines.push(Line::from(path_spans));

    // Show path suggestions when path field is active
    if field == NewWorktreeField::Path && !path_suggestions.is_empty() {
        lines.push(Line::styled(
            "         ────────────────────────────────────",
            Style::default().fg(Color::DarkGray),
        ));

        for (i, suggestion) in path_suggestions.iter().take(5).enumerate() {
            let is_selected = path_selected == Some(i);
            let prefix = if is_selected {
                "       > "
            } else {
                "         "
            };
            let style = if is_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            lines.push(Line::styled(format!("{}{}", prefix, suggestion), style));
        }

        if path_suggestions.len() > 5 {
            lines.push(Line::styled(
                format!("         ... and {} more", path_suggestions.len() - 5),
                Style::default().fg(Color::DarkGray),
            ));
        }

        lines.push(Line::styled(
            "         ────────────────────────────────────",
            Style::default().fg(Color::DarkGray),
        ));
    }

    lines.push(Line::raw(""));

    // Session name field
    let session_style = if field == NewWorktreeField::SessionName {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
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
        "Tab switch  ↑↓ select  → accept  Enter create  Esc cancel",
        Style::default().fg(Color::DarkGray),
    ));

    let text = Text::from(lines);
    let paragraph = Paragraph::new(text)
        .block(block)
        .wrap(Wrap { trim: false });

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

pub fn render_rename_dialog(frame: &mut Frame, old_name: &str, new_name: &str) {
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
