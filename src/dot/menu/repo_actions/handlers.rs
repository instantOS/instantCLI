use anyhow::Result;
use std::path::PathBuf;

use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::repo::{RepositoryManager, cli::RepoCommands};
use crate::menu_utils::{ConfirmResult, FzfResult, FzfWrapper, Header, MenuCursor};
use crate::ui::catppuccin::fzf_mocha_args;

use super::super::subdir_actions::handle_manage_subdirs;
use super::action_menu::{RepoAction, build_repo_action_menu};
use super::details::handle_edit_details;
use super::preview::build_repo_preview;

/// Handle repo actions
pub fn handle_repo_actions(
    repo_name: &str,
    config: &mut Config,
    db: &Database,
    debug: bool,
) -> Result<()> {
    let mut cursor = MenuCursor::new();

    loop {
        let Some(action) = select_repo_action(repo_name, config, db, &mut cursor)? else {
            return Ok(());
        };

        if dispatch_repo_action(action, repo_name, config, db, debug)? {
            return Ok(());
        }
    }
}

fn select_repo_action(
    repo_name: &str,
    config: &Config,
    db: &Database,
    cursor: &mut MenuCursor,
) -> Result<Option<RepoAction>> {
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

    match result {
        FzfResult::Selected(item) => {
            cursor.update(&item, &actions);
            Ok(Some(item.action))
        }
        FzfResult::Cancelled => Ok(None),
        _ => Ok(None),
    }
}

fn dispatch_repo_action(
    action: RepoAction,
    repo_name: &str,
    config: &mut Config,
    db: &Database,
    debug: bool,
) -> Result<bool> {
    match action {
        RepoAction::Toggle => toggle_repo(repo_name, config, db, debug)?,
        RepoAction::BumpPriority => {
            handle_priority_change(repo_name, config.move_repo_up(repo_name, None))?;
        }
        RepoAction::LowerPriority => {
            handle_priority_change(repo_name, config.move_repo_down(repo_name, None))?;
        }
        RepoAction::ManageSubdirs => {
            handle_manage_subdirs(repo_name, config, db, debug)?;
        }
        RepoAction::EditDetails => {
            handle_edit_details(repo_name, config, db)?;
        }
        RepoAction::ShowInfo => {
            show_repo_info(repo_name, config, db)?;
        }
        RepoAction::Remove => return remove_repo(repo_name, config, db, debug),
        RepoAction::Back => return Ok(true),
        RepoAction::OpenInLazygit => open_repo_lazygit(repo_name, config, db)?,
        RepoAction::OpenInShell => open_repo_shell(repo_name, config, db)?,
        RepoAction::ToggleReadOnly => toggle_read_only(repo_name, config)?,
    }

    Ok(false)
}

fn toggle_repo(repo_name: &str, config: &mut Config, db: &Database, debug: bool) -> Result<()> {
    let is_enabled = config
        .repos
        .iter()
        .find(|r| r.name == repo_name)
        .map(|r| r.enabled)
        .unwrap_or(false);

    set_repo_enabled(repo_name, config, db, debug, !is_enabled)
}

fn set_repo_enabled(
    repo_name: &str,
    config: &mut Config,
    db: &Database,
    debug: bool,
    enabled: bool,
) -> Result<()> {
    let command = if enabled {
        RepoCommands::Enable {
            name: repo_name.to_string(),
        }
    } else {
        RepoCommands::Disable {
            name: repo_name.to_string(),
        }
    };

    crate::dot::repo::commands::handle_repo_command(config, db, &command, debug)?;

    let status = if enabled { "enabled" } else { "disabled" };
    FzfWrapper::message(&format!("Repository '{}' has been {}", repo_name, status))?;
    Ok(())
}

fn handle_priority_change(repo_name: &str, result: Result<usize>) -> Result<()> {
    match result {
        Ok(new_pos) => {
            FzfWrapper::message(&format!(
                "Repository '{}' moved to priority P{}",
                repo_name, new_pos
            ))?;
        }
        Err(e) => {
            FzfWrapper::message(&format!("Error: {}", e))?;
        }
    }
    Ok(())
}

fn show_repo_info(repo_name: &str, config: &Config, db: &Database) -> Result<()> {
    let info_text = build_repo_preview(repo_name, config, db);

    FzfWrapper::builder()
        .message(&info_text)
        .title(repo_name)
        .message_dialog()?;
    Ok(())
}

fn remove_repo(repo_name: &str, config: &mut Config, db: &Database, debug: bool) -> Result<bool> {
    let confirm = FzfWrapper::builder()
        .confirm(format!(
            "Remove repository '{}'?\n\nThis will remove it from your configuration.",
            repo_name
        ))
        .yes_text("Remove")
        .no_text("Cancel")
        .confirm_dialog()?;

    if !matches!(confirm, ConfirmResult::Yes) {
        return Ok(false);
    }

    let keep_files_result = FzfWrapper::builder()
        .confirm("Keep local files?")
        .yes_text("Keep Files")
        .no_text("Remove Files")
        .confirm_dialog()?;

    let keep_files = matches!(keep_files_result, ConfirmResult::Yes);

    let command = RepoCommands::Remove {
        name: repo_name.to_string(),
        keep_files,
    };
    crate::dot::repo::commands::handle_repo_command(config, db, &command, debug)?;
    Ok(true)
}

fn open_repo_lazygit(repo_name: &str, config: &Config, db: &Database) -> Result<()> {
    if let Some(repo_path) = repo_path_if_available(repo_name, config, db) {
        std::process::Command::new("lazygit")
            .current_dir(&repo_path)
            .status()?;
    }
    Ok(())
}

fn open_repo_shell(repo_name: &str, config: &Config, db: &Database) -> Result<()> {
    if let Some(repo_path) = repo_path_if_available(repo_name, config, db) {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "bash".to_string());
        std::process::Command::new(shell)
            .current_dir(&repo_path)
            .status()?;
    }
    Ok(())
}

fn repo_path_if_available(repo_name: &str, config: &Config, db: &Database) -> Option<PathBuf> {
    let repo_manager = RepositoryManager::new(config, db);
    let local_repo = repo_manager.get_repository_info(repo_name).ok()?;
    local_repo.local_path(config).ok()
}

fn toggle_read_only(repo_name: &str, config: &mut Config) -> Result<()> {
    let is_read_only = config
        .repos
        .iter()
        .find(|r| r.name == repo_name)
        .map(|r| r.read_only)
        .unwrap_or(false);

    if is_read_only {
        if confirm_make_writable(repo_name)? {
            crate::dot::repo::commands::set_read_only_status(config, repo_name, false)?;
            FzfWrapper::message(&format!("Repository '{}' is now writable", repo_name))?;
        }
    } else {
        crate::dot::repo::commands::set_read_only_status(config, repo_name, true)?;
        FzfWrapper::message(&format!("Repository '{}' is now read-only", repo_name))?;
    }

    Ok(())
}

fn confirm_make_writable(repo_name: &str) -> Result<bool> {
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

    Ok(matches!(confirm, ConfirmResult::Yes))
}
