use anyhow::Result;

use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::repo::{cli::RepoCommands, RepositoryManager};
use crate::menu_utils::{ConfirmResult, FzfResult, FzfWrapper, Header, MenuCursor};
use crate::ui::catppuccin::fzf_mocha_args;

use super::action_menu::{build_repo_action_menu, RepoAction};
use super::details::handle_edit_details;
use super::preview::build_repo_preview;
use super::super::subdir_actions::handle_manage_subdirs;

/// Handle repo actions
pub fn handle_repo_actions(
    repo_name: &str,
    config: &mut Config,
    db: &Database,
    debug: bool,
) -> Result<()> {
    let mut cursor = MenuCursor::new();

    loop {
        let actions = build_repo_action_menu(repo_name, config, db);

        let mut builder = FzfWrapper::builder()
            .header(Header::fancy(&format!("Repository: {}", repo_name)))
            .prompt("Select action")
            .args(fzf_mocha_args())
            .responsive_layout();

        if let Some(index) = cursor.initial_index(&actions) {
            builder = builder.initial_index(index);
        }

        let result = builder.select_padded(actions.clone())?;

        let action = match result {
            FzfResult::Selected(item) => {
                cursor.update(&item, &actions);
                item.action
            }
            FzfResult::Cancelled => return Ok(()),
            _ => return Ok(()),
        };

        match action {
            RepoAction::Toggle => {
                // Determine current state and toggle
                let is_enabled = config
                    .repos
                    .iter()
                    .find(|r| r.name == repo_name)
                    .map(|r| r.enabled)
                    .unwrap_or(false);

                if is_enabled {
                    let clone_args = RepoCommands::Disable {
                        name: repo_name.to_string(),
                    };
                    crate::dot::repo::commands::handle_repo_command(
                        config,
                        db,
                        &clone_args,
                        debug,
                    )?;
                    FzfWrapper::message(&format!("Repository '{}' has been disabled", repo_name))?;
                } else {
                    let clone_args = RepoCommands::Enable {
                        name: repo_name.to_string(),
                    };
                    crate::dot::repo::commands::handle_repo_command(
                        config,
                        db,
                        &clone_args,
                        debug,
                    )?;
                    FzfWrapper::message(&format!("Repository '{}' has been enabled", repo_name))?;
                }
            }
            RepoAction::BumpPriority => match config.move_repo_up(repo_name, None) {
                Ok(new_pos) => {
                    FzfWrapper::message(&format!(
                        "Repository '{}' moved to priority P{}",
                        repo_name, new_pos
                    ))?;
                }
                Err(e) => {
                    FzfWrapper::message(&format!("Error: {}", e))?;
                }
            },
            RepoAction::LowerPriority => match config.move_repo_down(repo_name, None) {
                Ok(new_pos) => {
                    FzfWrapper::message(&format!(
                        "Repository '{}' moved to priority P{}",
                        repo_name, new_pos
                    ))?;
                }
                Err(e) => {
                    FzfWrapper::message(&format!("Error: {}", e))?;
                }
            },
            RepoAction::ManageSubdirs => {
                handle_manage_subdirs(repo_name, config, db, debug)?;
            }
            RepoAction::EditDetails => {
                handle_edit_details(repo_name, config, db)?;
            }
            RepoAction::ShowInfo => {
                // Build the info string using the preview builder
                let info_text = build_repo_preview(repo_name, config, db);

                // Display in a message dialog
                FzfWrapper::builder()
                    .message(&info_text)
                    .title(repo_name)
                    .message_dialog()?;
            }
            RepoAction::Remove => {
                let confirm = FzfWrapper::builder()
                    .confirm(format!(
                        "Remove repository '{}'?\n\nThis will remove it from your configuration.",
                        repo_name
                    ))
                    .yes_text("Remove")
                    .no_text("Cancel")
                    .confirm_dialog()?;

                if matches!(confirm, ConfirmResult::Yes) {
                    // Ask if we should keep files
                    let keep_files_result = FzfWrapper::builder()
                        .confirm("Keep local files?")
                        .yes_text("Keep Files")
                        .no_text("Remove Files")
                        .confirm_dialog()?;

                    let keep_files = matches!(keep_files_result, ConfirmResult::Yes);

                    let clone_args = RepoCommands::Remove {
                        name: repo_name.to_string(),
                        keep_files,
                    };
                    crate::dot::repo::commands::handle_repo_command(
                        config,
                        db,
                        &clone_args,
                        debug,
                    )?;
                    return Ok(()); // Exit repo menu after removing
                }
            }
            RepoAction::Back => return Ok(()),
            RepoAction::OpenInLazygit => {
                let repo_manager = RepositoryManager::new(config, db);
                if let Ok(local_repo) = repo_manager.get_repository_info(repo_name)
                    && let Ok(repo_path) = local_repo.local_path(config)
                {
                    // Spawn lazygit in the repo directory
                    std::process::Command::new("lazygit")
                        .current_dir(&repo_path)
                        .status()?;
                }
            }
            RepoAction::OpenInShell => {
                let repo_manager = RepositoryManager::new(config, db);
                if let Ok(local_repo) = repo_manager.get_repository_info(repo_name)
                    && let Ok(repo_path) = local_repo.local_path(config)
                {
                    let shell = std::env::var("SHELL").unwrap_or_else(|_| "bash".to_string());
                    std::process::Command::new(shell)
                        .current_dir(&repo_path)
                        .status()?;
                }
            }
            RepoAction::ToggleReadOnly => {
                let is_read_only = config
                    .repos
                    .iter()
                    .find(|r| r.name == repo_name)
                    .map(|r| r.read_only)
                    .unwrap_or(false);

                if is_read_only {
                    // Making writable - show warning
                    let confirm = FzfWrapper::builder()
                        .confirm(format!(
                            "Make '{}' writable?\n\n\
 ⚠️  WARNING: This will allow the repository to diverge from upstream.\n\
 You may be unable to receive updates without manual work.\n\n\
 Consider adding your own dotfile repository on top instead.\n\
 See: https://instantos.io/docs/insdot.html",
                            repo_name
                        ))
                        .yes_text("Make Writable")
                        .no_text("Cancel")
                        .confirm_dialog()?;

                    if matches!(confirm, ConfirmResult::Yes) {
                        crate::dot::repo::commands::set_read_only_status(config, repo_name, false)?;
                        FzfWrapper::message(&format!(
                            "Repository '{}' is now writable",
                            repo_name
                        ))?;
                    }
                } else {
                    // Making read-only
                    crate::dot::repo::commands::set_read_only_status(config, repo_name, true)?;
                    FzfWrapper::message(&format!("Repository '{}' is now read-only", repo_name))?;
                }
            }
        }
    }
}
