use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, CreatePullRequestField, Mode, NewSessionField, NewWorktreeField};

/// Handle a key event and update the application state
pub fn handle_key(app: &mut App, key: KeyEvent) {
    // Clear messages on any key press
    app.clear_messages();

    match &app.mode {
        Mode::Normal => handle_normal_mode(app, key),
        Mode::ActionMenu => handle_action_menu_mode(app, key),
        Mode::Filter { .. } => handle_filter_mode(app, key),
        Mode::ConfirmAction => handle_confirm_action_mode(app, key),
        Mode::NewSession { .. } => handle_new_session_mode(app, key),
        Mode::Rename { .. } => handle_rename_mode(app, key),
        Mode::Commit { .. } => handle_commit_mode(app, key),
        Mode::NewWorktree { .. } => handle_new_worktree_mode(app, key),
        Mode::CreatePullRequest { .. } => handle_create_pr_mode(app, key),
        Mode::Help => handle_help_mode(app, key),
    }
}

fn handle_normal_mode(app: &mut App, key: KeyEvent) {
    match key.code {
        // Quit
        KeyCode::Char('q') | KeyCode::Esc => {
            app.should_quit = true;
        }

        // Navigation
        KeyCode::Char('j') | KeyCode::Down => {
            app.select_next();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.select_prev();
        }

        // Enter action menu
        KeyCode::Char('l') | KeyCode::Right => {
            app.enter_action_menu();
        }

        // Switch to session (quick action)
        KeyCode::Enter => {
            app.switch_to_selected();
        }

        // New session
        KeyCode::Char('n') => {
            app.start_new_session();
        }

        // Kill session (capital K to avoid accidents)
        KeyCode::Char('K') => {
            app.start_kill();
        }

        // Rename session
        KeyCode::Char('r') => {
            app.start_rename();
        }

        // Filter
        KeyCode::Char('/') => {
            app.start_filter();
        }

        // Clear filter
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.clear_filter();
        }

        // Refresh
        KeyCode::Char('R') => {
            app.refresh();
        }

        // Help
        KeyCode::Char('?') => {
            app.show_help();
        }

        _ => {}
    }
}

fn handle_filter_mode(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.cancel();
        }
        KeyCode::Enter => {
            app.apply_filter();
        }
        KeyCode::Backspace => {
            if let Mode::Filter { ref mut input } = app.mode {
                input.pop();
            }
        }
        KeyCode::Char(c) => {
            if let Mode::Filter { ref mut input } = app.mode {
                input.push(c);
            }
        }
        _ => {}
    }
}

fn handle_action_menu_mode(app: &mut App, key: KeyEvent) {
    match key.code {
        // Navigate actions
        KeyCode::Char('j') | KeyCode::Down => {
            app.select_next_action();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.select_prev_action();
        }

        // Execute selected action
        KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => {
            app.execute_selected_action();
        }

        // Back to session list
        KeyCode::Char('h') | KeyCode::Left | KeyCode::Esc => {
            app.cancel();
        }

        // Quit entirely
        KeyCode::Char('q') => {
            app.should_quit = true;
        }

        _ => {}
    }
}

fn handle_confirm_action_mode(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
            app.confirm_action();
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.cancel();
        }
        _ => {}
    }
}

fn handle_new_session_mode(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.cancel();
        }
        KeyCode::Tab => {
            // Toggle between name and path fields
            if let Mode::NewSession { ref mut field, .. } = app.mode {
                *field = match field {
                    NewSessionField::Name => NewSessionField::Path,
                    NewSessionField::Path => NewSessionField::Name,
                };
            }
        }
        KeyCode::Enter => {
            app.confirm_new_session(true); // Start claude by default
        }
        KeyCode::Backspace => {
            if let Mode::NewSession {
                ref mut name,
                ref mut path,
                ref field,
            } = app.mode
            {
                match field {
                    NewSessionField::Name => {
                        name.pop();
                    }
                    NewSessionField::Path => {
                        path.pop();
                    }
                }
            }
        }
        KeyCode::Char(c) => {
            if let Mode::NewSession {
                ref mut name,
                ref mut path,
                ref field,
            } = app.mode
            {
                match field {
                    NewSessionField::Name => {
                        // Only allow valid session name characters
                        if c.is_alphanumeric() || c == '-' || c == '_' {
                            name.push(c);
                        }
                    }
                    NewSessionField::Path => {
                        path.push(c);
                    }
                }
            }
        }
        _ => {}
    }
}

fn handle_rename_mode(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.cancel();
        }
        KeyCode::Enter => {
            app.confirm_rename();
        }
        KeyCode::Backspace => {
            if let Mode::Rename { ref mut new_name, .. } = app.mode {
                new_name.pop();
            }
        }
        KeyCode::Char(c) => {
            if let Mode::Rename { ref mut new_name, .. } = app.mode {
                // Only allow valid session name characters
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    new_name.push(c);
                }
            }
        }
        _ => {}
    }
}

fn handle_commit_mode(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.cancel();
        }
        KeyCode::Enter => {
            app.confirm_commit();
        }
        KeyCode::Backspace => {
            if let Mode::Commit { ref mut message } = app.mode {
                message.pop();
            }
        }
        KeyCode::Char(c) => {
            if let Mode::Commit { ref mut message } = app.mode {
                message.push(c);
            }
        }
        _ => {}
    }
}

fn handle_new_worktree_mode(app: &mut App, key: KeyEvent) {
    // Get current field to determine behavior
    let current_field = if let Mode::NewWorktree { field, .. } = &app.mode {
        *field
    } else {
        return;
    };

    match key.code {
        KeyCode::Esc => {
            app.cancel();
        }
        KeyCode::Tab => {
            // Cycle through fields
            if let Mode::NewWorktree { ref mut field, .. } = app.mode {
                *field = match field {
                    NewWorktreeField::Branch => NewWorktreeField::Path,
                    NewWorktreeField::Path => NewWorktreeField::SessionName,
                    NewWorktreeField::SessionName => NewWorktreeField::Branch,
                };
            }
        }
        KeyCode::BackTab => {
            // Cycle backwards through fields
            if let Mode::NewWorktree { ref mut field, .. } = app.mode {
                *field = match field {
                    NewWorktreeField::Branch => NewWorktreeField::SessionName,
                    NewWorktreeField::Path => NewWorktreeField::Branch,
                    NewWorktreeField::SessionName => NewWorktreeField::Path,
                };
            }
        }
        KeyCode::Enter => {
            app.confirm_new_worktree();
        }
        KeyCode::Backspace => {
            if let Mode::NewWorktree {
                ref mut branch_input,
                ref mut worktree_path,
                ref mut session_name,
                field,
                ..
            } = app.mode
            {
                match field {
                    NewWorktreeField::Branch => {
                        branch_input.pop();
                    }
                    NewWorktreeField::Path => {
                        worktree_path.pop();
                    }
                    NewWorktreeField::SessionName => {
                        session_name.pop();
                    }
                }
            }
            // Update suggestions after branch input changes
            if current_field == NewWorktreeField::Branch {
                app.update_worktree_suggestions();
            }
        }
        KeyCode::Char(c) => {
            if let Mode::NewWorktree {
                ref mut branch_input,
                ref mut worktree_path,
                ref mut session_name,
                field,
                ..
            } = app.mode
            {
                match field {
                    NewWorktreeField::Branch => {
                        branch_input.push(c);
                    }
                    NewWorktreeField::Path => {
                        worktree_path.push(c);
                    }
                    NewWorktreeField::SessionName => {
                        // Only allow valid session name characters
                        if c.is_alphanumeric() || c == '-' || c == '_' {
                            session_name.push(c);
                        }
                    }
                }
            }
            // Update suggestions after branch input changes
            if current_field == NewWorktreeField::Branch {
                app.update_worktree_suggestions();
            }
        }
        // Navigate branch suggestions when in Branch field
        KeyCode::Down if current_field == NewWorktreeField::Branch => {
            let filtered_count = app.filtered_branches().len();
            if filtered_count > 0 {
                if let Mode::NewWorktree {
                    ref mut selected_branch,
                    ..
                } = app.mode
                {
                    *selected_branch =
                        Some(selected_branch.map(|i| (i + 1) % filtered_count).unwrap_or(0));
                }
                app.update_worktree_suggestions();
            }
        }
        KeyCode::Up if current_field == NewWorktreeField::Branch => {
            let filtered_count = app.filtered_branches().len();
            if filtered_count > 0 {
                if let Mode::NewWorktree {
                    ref mut selected_branch,
                    ..
                } = app.mode
                {
                    *selected_branch = Some(
                        selected_branch
                            .map(|i| if i == 0 { filtered_count - 1 } else { i - 1 })
                            .unwrap_or(filtered_count - 1),
                    );
                }
                app.update_worktree_suggestions();
            }
        }
        _ => {}
    }
}

fn handle_create_pr_mode(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.cancel();
        }
        KeyCode::Tab => {
            // Cycle through fields
            if let Mode::CreatePullRequest { ref mut field, .. } = app.mode {
                *field = match field {
                    CreatePullRequestField::Title => CreatePullRequestField::Body,
                    CreatePullRequestField::Body => CreatePullRequestField::BaseBranch,
                    CreatePullRequestField::BaseBranch => CreatePullRequestField::Title,
                };
            }
        }
        KeyCode::BackTab => {
            // Cycle backwards through fields
            if let Mode::CreatePullRequest { ref mut field, .. } = app.mode {
                *field = match field {
                    CreatePullRequestField::Title => CreatePullRequestField::BaseBranch,
                    CreatePullRequestField::Body => CreatePullRequestField::Title,
                    CreatePullRequestField::BaseBranch => CreatePullRequestField::Body,
                };
            }
        }
        KeyCode::Enter => {
            app.confirm_create_pull_request();
        }
        KeyCode::Backspace => {
            if let Mode::CreatePullRequest {
                ref mut title,
                ref mut body,
                ref mut base_branch,
                field,
            } = app.mode
            {
                match field {
                    CreatePullRequestField::Title => {
                        title.pop();
                    }
                    CreatePullRequestField::Body => {
                        body.pop();
                    }
                    CreatePullRequestField::BaseBranch => {
                        base_branch.pop();
                    }
                }
            }
        }
        KeyCode::Char(c) => {
            if let Mode::CreatePullRequest {
                ref mut title,
                ref mut body,
                ref mut base_branch,
                field,
            } = app.mode
            {
                match field {
                    CreatePullRequestField::Title => {
                        title.push(c);
                    }
                    CreatePullRequestField::Body => {
                        body.push(c);
                    }
                    CreatePullRequestField::BaseBranch => {
                        // Branch names have specific allowed characters
                        if c.is_alphanumeric() || c == '-' || c == '_' || c == '/' {
                            base_branch.push(c);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn handle_help_mode(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc | KeyCode::Char('?') => {
            app.cancel();
        }
        _ => {}
    }
}
