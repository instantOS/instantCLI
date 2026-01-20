//! Repository action handling for the dot menu

use anyhow::Result;

use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::repo::{RepositoryManager, cli::RepoCommands};
use crate::menu_utils::{ConfirmResult, FzfResult, FzfSelectable, FzfWrapper, Header, MenuCursor};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::subdir_actions::handle_manage_subdirs;

/// Repo action for individual repository menu
#[derive(Debug, Clone)]
pub enum RepoAction {
    Toggle,
    BumpPriority,
    LowerPriority,
    ManageSubdirs,
    ToggleReadOnly,
    OpenInLazygit,
    OpenInShell,
    ShowInfo,
    Remove,
    Back,
}

#[derive(Clone)]
pub struct RepoActionItem {
    display: String,
    preview: String,
    pub action: RepoAction,
}

impl FzfSelectable for RepoActionItem {
    fn fzf_display_text(&self) -> String {
        self.display.clone()
    }

    fn fzf_key(&self) -> String {
        self.display.clone()
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        crate::menu::protocol::FzfPreview::Text(self.preview.clone())
    }
}

/// Build the repo action menu items
pub fn build_repo_action_menu(repo_name: &str, config: &Config) -> Vec<RepoActionItem> {
    let repo_config = config.repos.iter().find(|r| r.name == repo_name);

    let is_enabled = repo_config.map(|r| r.enabled).unwrap_or(false);

    // Find current priority position (1-indexed)
    let current_position = config
        .repos
        .iter()
        .position(|r| r.name == repo_name)
        .map(|i| i + 1)
        .unwrap_or(1);
    let total_repos = config.repos.len();

    let mut actions = Vec::new();

    // Toggle enable/disable
    let (icon, color, text, preview) = if is_enabled {
        (
            NerdFont::ToggleOff,
            colors::RED,
            "Disable",
            format!(
                "Disable '{}'.\n\nDisabled repositories won't be applied during 'ins dot apply'.",
                repo_name
            ),
        )
    } else {
        (
            NerdFont::ToggleOn,
            colors::GREEN,
            "Enable",
            format!(
                "Enable '{}'.\n\nEnabled repositories will be applied during 'ins dot apply'.",
                repo_name
            ),
        )
    };

    actions.push(RepoActionItem {
        display: format!("{} {}", format_icon_colored(icon, color), text),
        preview,
        action: RepoAction::Toggle,
    });

    // Priority: Bump up (only if not already at top)
    if current_position > 1 {
        actions.push(RepoActionItem {
            display: format!(
                "{} Bump Priority",
                format_icon_colored(NerdFont::ArrowUp, colors::PEACH)
            ),
            preview: format!(
                "Move '{}' up in priority.\n\nCurrent: P{} → New: P{}\n\nHigher priority repos override lower ones for the same file.",
                repo_name,
                current_position,
                current_position - 1
            ),
            action: RepoAction::BumpPriority,
        });
    }

    // Priority: Lower down (only if not already at bottom)
    if current_position < total_repos {
        actions.push(RepoActionItem {
            display: format!(
                "{} Lower Priority",
                format_icon_colored(NerdFont::ArrowDown, colors::LAVENDER)
            ),
            preview: format!(
                "Move '{}' down in priority.\n\nCurrent: P{} → New: P{}\n\nHigher priority repos override lower ones for the same file.",
                repo_name,
                current_position,
                current_position + 1
            ),
            action: RepoAction::LowerPriority,
        });
    }

    // Manage subdirs
    actions.push(RepoActionItem {
        display: format!(
            "{} Manage Subdirs",
            format_icon_colored(NerdFont::Folder, colors::MAUVE)
        ),
        preview: format!(
            "Manage dotfile directories for '{}'.\n\nEnable or disable specific subdirectories within this repository.",
            repo_name
        ),
        action: RepoAction::ManageSubdirs,
    });

    // Toggle read-only
    let is_read_only = repo_config.map(|r| r.read_only).unwrap_or(false);
    let (ro_icon, ro_color, ro_text, ro_preview) = if is_read_only {
        (
            NerdFont::Lock,
            colors::YELLOW,
            "Make Writable",
            format!(
                "Make '{}' writable.\n\n\
⚠️  WARNING: This will allow the repository to diverge from upstream.\n\
You may be unable to receive updates without manual work.\n\n\
Consider adding your own dotfile repository on top instead.\n\
See: https://instantos.io/docs/insdot.html",
                repo_name
            ),
        )
    } else {
        (
            NerdFont::Lock,
            colors::GREEN,
            "Make Read-Only",
            format!(
                "Make '{}' read-only.\n\n\
Read-only repositories cannot be modified by 'ins dot add'.\n\
This helps keep the repository in sync with upstream.",
                repo_name
            ),
        )
    };

    actions.push(RepoActionItem {
        display: format!("{} {}", format_icon_colored(ro_icon, ro_color), ro_text),
        preview: ro_preview,
        action: RepoAction::ToggleReadOnly,
    });

    // Open in Lazygit
    actions.push(RepoActionItem {
        display: format!(
            "{} Open in Lazygit",
            format_icon_colored(NerdFont::GitBranch, colors::PEACH)
        ),
        preview: format!(
            "Open '{}' in Lazygit.\n\nLazygit is a terminal UI for git commands.\nYou can view commits, branches, and manage the repository.",
            repo_name
        ),
        action: RepoAction::OpenInLazygit,
    });

    // Open in Shell
    actions.push(RepoActionItem {
        display: format!(
            "{} Open in Shell",
            format_icon_colored(NerdFont::Terminal, colors::GREEN)
        ),
        preview: format!(
            "Open a new shell in '{}'.\n\nYou can use this to manually browse or modify files in the repository.",
            repo_name
        ),
        action: RepoAction::OpenInShell,
    });

    // Show info
    actions.push(RepoActionItem {
        display: format!(
            "{} Show Info",
            format_icon_colored(NerdFont::Info, colors::BLUE)
        ),
        preview: format!(
            "Show detailed information about '{}'.\n\nDisplay repository URL, branch, subdirectories, and local path.",
            repo_name
        ),
        action: RepoAction::ShowInfo,
    });

    // Remove
    actions.push(RepoActionItem {
        display: format!(
            "{} Remove",
            format_icon_colored(NerdFont::Trash, colors::RED)
        ),
        preview: format!(
            "Remove '{}' from your configuration.\n\nYou'll be asked whether to:\n• Keep files (just remove from config)\n• Delete files (remove from disk too)",
            repo_name
        ),
        action: RepoAction::Remove,
    });

    // Back
    actions.push(RepoActionItem {
        display: format!("{} Back", format_back_icon()),
        preview: "Return to repository selection".to_string(),
        action: RepoAction::Back,
    });

    actions
}

/// Build preview for a repository in the main menu
pub fn build_repo_preview(repo_name: &str, config: &Config, db: &Database) -> String {
    let repo_manager = RepositoryManager::new(config, db);

    let repo_config = match config.repos.iter().find(|r| r.name == repo_name) {
        Some(rc) => rc,
        None => return format!("Repository '{}' not found in config", repo_name),
    };

    let mut builder = PreviewBuilder::new().title(colors::SKY, repo_name).blank();

    // Show external repo status if applicable
    if repo_config.metadata.is_some() {
        builder = builder.line(
            colors::YELLOW,
            Some(NerdFont::Info),
            "External (Yadm/Stow compatible - metadata in config)",
        );
    }

    builder = builder.line(
        colors::TEXT,
        Some(NerdFont::Link),
        &format!("URL: {}", repo_config.url),
    );

    // Branch
    if let Some(branch) = &repo_config.branch {
        builder = builder.line(
            colors::TEXT,
            Some(NerdFont::GitBranch),
            &format!("Branch: {}", branch),
        );
    }

    // Priority
    let priority = config
        .repos
        .iter()
        .position(|r| r.name == repo_name)
        .map(|i| i + 1)
        .unwrap_or(0);
    let total_repos = config.repos.len();

    if priority > 0 {
        let label = if priority == 1 && total_repos > 1 {
            " (highest priority)"
        } else if priority == total_repos && total_repos > 1 {
            " (lowest priority)"
        } else {
            ""
        };

        builder = builder.line(
            colors::PEACH,
            Some(NerdFont::ArrowUp),
            &format!("Priority: P{}{}", priority, label),
        );
    }

    // Status
    let status_color = if repo_config.enabled {
        colors::GREEN
    } else {
        colors::RED
    };
    let status_text = if repo_config.enabled {
        "Enabled"
    } else {
        "Disabled"
    };
    let status_icon = if repo_config.enabled {
        NerdFont::ToggleOn
    } else {
        NerdFont::ToggleOff
    };
    builder = builder.line(status_color, Some(status_icon), status_text);

    // Read-only
    if repo_config.read_only {
        builder = builder.line(colors::YELLOW, Some(NerdFont::Lock), "Read-only");
    }

    // Try to get more info from LocalRepo
    if let Ok(local_repo) = repo_manager.get_repository_info(repo_name) {
        builder = builder
            .blank()
            .line(colors::MAUVE, Some(NerdFont::Folder), "Subdirectories");

        if local_repo.meta.dots_dirs.is_empty() {
            builder = builder.indented_line(colors::SUBTEXT0, None, "No subdirectories configured");
        } else {
            let available = local_repo.meta.dots_dirs.join(", ");
            let active = if let Some(active_subdirs) = &repo_config.active_subdirectories {
                if active_subdirs.is_empty() {
                    "(none configured)".to_string()
                } else {
                    active_subdirs.join(", ")
                }
            } else if local_repo.meta.dots_dirs.is_empty() {
                "(none configured)".to_string()
            } else {
                let repo_path = config.repos_path().join(&repo_config.name);
                let effective_active = config.resolve_active_subdirs(repo_config);
                if effective_active.is_empty() {
                    if repo_path.join("instantdots.toml").exists() || repo_config.metadata.is_some()
                    {
                        "(none configured)".to_string()
                    } else {
                        "(none detected)".to_string()
                    }
                } else {
                    effective_active.join(", ")
                }
            };
            builder = builder
                .indented_line(colors::TEXT, None, &format!("Available: {}", available))
                .indented_line(colors::GREEN, None, &format!("Active: {}", active));
        }

        // Local path
        if let Ok(local_path) = local_repo.local_path(config) {
            let tilde_path = local_path.display().to_string();
            builder = builder.blank().indented_line(
                colors::TEXT,
                Some(NerdFont::Folder),
                &format!("Local: {}", tilde_path),
            );
        }
    }

    builder.build_string()
}

/// Handle repo actions
pub fn handle_repo_actions(
    repo_name: &str,
    _config: &Config,
    _db: &Database,
    debug: bool,
) -> Result<()> {
    let mut cursor = MenuCursor::new();

    loop {
        let config = Config::load(None)?;
        let db = Database::new(config.database_path().to_path_buf())?;
        let actions = build_repo_action_menu(repo_name, &config);

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

                let mut config = Config::load(None)?;
                let db = Database::new(config.database_path().to_path_buf())?;

                if is_enabled {
                    let clone_args = RepoCommands::Disable {
                        name: repo_name.to_string(),
                    };
                    crate::dot::repo::commands::handle_repo_command(
                        &mut config,
                        &db,
                        &clone_args,
                        debug,
                    )?;
                    FzfWrapper::message(&format!("Repository '{}' has been disabled", repo_name))?;
                } else {
                    let clone_args = RepoCommands::Enable {
                        name: repo_name.to_string(),
                    };
                    crate::dot::repo::commands::handle_repo_command(
                        &mut config,
                        &db,
                        &clone_args,
                        debug,
                    )?;
                    FzfWrapper::message(&format!("Repository '{}' has been enabled", repo_name))?;
                }
            }
            RepoAction::BumpPriority => {
                let mut config = Config::load(None)?;
                match config.move_repo_up(repo_name, None) {
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
            }
            RepoAction::LowerPriority => {
                let mut config = Config::load(None)?;
                match config.move_repo_down(repo_name, None) {
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
            }
            RepoAction::ManageSubdirs => {
                handle_manage_subdirs(repo_name, &config, &db, debug)?;
            }
            RepoAction::ShowInfo => {
                // Build the info string using the preview builder
                let info_text = build_repo_preview(repo_name, &config, &db);

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
                    let mut config = Config::load(None)?;
                    let db = Database::new(config.database_path().to_path_buf())?;

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
                        &mut config,
                        &db,
                        &clone_args,
                        debug,
                    )?;
                    return Ok(()); // Exit repo menu after removing
                }
            }
            RepoAction::Back => return Ok(()),
            RepoAction::OpenInLazygit => {
                let repo_manager = RepositoryManager::new(&config, &db);
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
                let repo_manager = RepositoryManager::new(&config, &db);
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
                        let mut config = Config::load(None)?;
                        crate::dot::repo::commands::set_read_only_status(
                            &mut config,
                            repo_name,
                            false,
                        )?;
                        FzfWrapper::message(&format!(
                            "Repository '{}' is now writable",
                            repo_name
                        ))?;
                    }
                } else {
                    // Making read-only
                    let mut config = Config::load(None)?;
                    crate::dot::repo::commands::set_read_only_status(&mut config, repo_name, true)?;
                    FzfWrapper::message(&format!("Repository '{}' is now read-only", repo_name))?;
                }
            }
        }
    }
}
