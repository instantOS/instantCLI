//! Subdirectory action handling for the dot menu

use anyhow::Result;

use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::localrepo::LocalRepo;
use crate::dot::repo::cli::RepoCommands;
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, Header};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

/// Subdir action for individual subdirectory menu
#[derive(Debug, Clone)]
enum SubdirAction {
    Toggle,
    BumpPriority,
    LowerPriority,
    Delete,
    Back,
}

#[derive(Clone)]
struct SubdirActionItem {
    display: String,
    preview: String,
    action: SubdirAction,
}

impl FzfSelectable for SubdirActionItem {
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

/// Build the subdir action menu items
fn build_subdir_action_menu(
    repo_name: &str,
    subdir_name: &str,
    config: &Config,
) -> Vec<SubdirActionItem> {
    let active_subdirs = config.get_active_subdirs(repo_name);
    let is_active = active_subdirs.contains(&subdir_name.to_string());

    // Find current priority position (1-indexed)
    let current_position = active_subdirs
        .iter()
        .position(|s| s == subdir_name)
        .map(|i| i + 1);
    let total_active = active_subdirs.len();

    let mut actions = Vec::new();

    // Toggle enable/disable
    let (icon, color, text, preview) = if is_active {
        (
            NerdFont::ToggleOff,
            colors::RED,
            "Disable",
            format!(
                "Disable '{}'.\n\nDisabled subdirectories won't be applied during 'ins dot apply'.",
                subdir_name
            ),
        )
    } else {
        (
            NerdFont::ToggleOn,
            colors::GREEN,
            "Enable",
            format!(
                "Enable '{}'.\n\nEnabled subdirectories will be applied during 'ins dot apply'.",
                subdir_name
            ),
        )
    };

    actions.push(SubdirActionItem {
        display: format!("{} {}", format_icon_colored(icon, color), text),
        preview,
        action: SubdirAction::Toggle,
    });

    // Priority options only for active subdirs with more than one active
    if let Some(pos) = current_position {
        // Priority: Bump up (only if not already at top and more than one active)
        if pos > 1 && total_active > 1 {
            actions.push(SubdirActionItem {
                display: format!(
                    "{} Bump Priority",
                    format_icon_colored(NerdFont::ArrowUp, colors::PEACH)
                ),
                preview: format!(
                    "Move '{}' up in priority.\n\nCurrent: P{} → New: P{}\n\nHigher priority subdirs override lower ones for the same file.",
                    subdir_name,
                    pos,
                    pos - 1
                ),
                action: SubdirAction::BumpPriority,
            });
        }

        // Priority: Lower down (only if not already at bottom and more than one active)
        if pos < total_active && total_active > 1 {
            actions.push(SubdirActionItem {
                display: format!(
                    "{} Lower Priority",
                    format_icon_colored(NerdFont::ArrowDown, colors::LAVENDER)
                ),
                preview: format!(
                    "Move '{}' down in priority.\n\nCurrent: P{} → New: P{}\n\nHigher priority subdirs override lower ones for the same file.",
                    subdir_name,
                    pos,
                    pos + 1
                ),
                action: SubdirAction::LowerPriority,
            });
        }
    }

    // Delete (only show if more than one subdir exists)
    let all_subdirs = &config
        .repos
        .iter()
        .find(|r| r.name == repo_name)
        .map(|r| r.active_subdirectories.len())
        .unwrap_or(0);

    // Check if this is the only subdir in the repo's instantdots.toml
    // We approximate this by checking if there are multiple subdirs total
    if *all_subdirs > 1 || !is_active {
        actions.push(SubdirActionItem {
            display: format!(
                "{} Delete",
                format_icon_colored(NerdFont::Trash, colors::RED)
            ),
            preview: format!(
                "Remove '{}' from this repository.\n\n\
                You'll be asked whether to:\n\
                • Keep files (just remove from config)\n\
                • Delete files (remove from disk too)",
                subdir_name
            ),
            action: SubdirAction::Delete,
        });
    }

    // Back
    actions.push(SubdirActionItem {
        display: format!("{} Back", format_back_icon()),
        preview: "Return to subdirectory selection".to_string(),
        action: SubdirAction::Back,
    });

    actions
}

/// Handle subdir actions
fn handle_subdir_actions(
    repo_name: &str,
    subdir_name: &str,
    _config: &Config,
    _db: &Database,
    debug: bool,
) -> Result<()> {
    loop {
        // Reload config to get current state
        let config = Config::load(None)?;
        let actions = build_subdir_action_menu(repo_name, subdir_name, &config);

        let result = FzfWrapper::builder()
            .header(Header::fancy(&format!("{} / {}", repo_name, subdir_name)))
            .prompt("Select action")
            .args(fzf_mocha_args())
            .responsive_layout()
            .select_padded(actions)?;

        let action = match result {
            FzfResult::Selected(item) => item.action,
            FzfResult::Cancelled => return Ok(()),
            _ => return Ok(()),
        };

        match action {
            SubdirAction::Toggle => {
                let active_subdirs = config.get_active_subdirs(repo_name);
                let is_active = active_subdirs.contains(&subdir_name.to_string());

                let mut config = Config::load(None)?;
                let db = Database::new(config.database_path().to_path_buf())?;

                let result = if is_active {
                    let clone_args = RepoCommands::Subdirs {
                        command: crate::dot::repo::cli::SubdirCommands::Disable {
                            name: repo_name.to_string(),
                            subdir: subdir_name.to_string(),
                        },
                    };
                    crate::dot::repo::commands::handle_repo_command(
                        &mut config,
                        &db,
                        &clone_args,
                        debug,
                    )
                } else {
                    let clone_args = RepoCommands::Subdirs {
                        command: crate::dot::repo::cli::SubdirCommands::Enable {
                            name: repo_name.to_string(),
                            subdir: subdir_name.to_string(),
                        },
                    };
                    crate::dot::repo::commands::handle_repo_command(
                        &mut config,
                        &db,
                        &clone_args,
                        debug,
                    )
                };

                if let Err(e) = result {
                    FzfWrapper::message(&format!("Error: {}", e))?;
                }
            }
            SubdirAction::BumpPriority => {
                let mut config = Config::load(None)?;
                match config.move_subdir_up(repo_name, subdir_name, None) {
                    Ok(new_pos) => {
                        FzfWrapper::message(&format!(
                            "Subdirectory '{}' moved to priority P{}",
                            subdir_name, new_pos
                        ))?;
                    }
                    Err(e) => {
                        FzfWrapper::message(&format!("Error: {}", e))?;
                    }
                }
            }
            SubdirAction::LowerPriority => {
                let mut config = Config::load(None)?;
                match config.move_subdir_down(repo_name, subdir_name, None) {
                    Ok(new_pos) => {
                        FzfWrapper::message(&format!(
                            "Subdirectory '{}' moved to priority P{}",
                            subdir_name, new_pos
                        ))?;
                    }
                    Err(e) => {
                        FzfWrapper::message(&format!("Error: {}", e))?;
                    }
                }
            }
            SubdirAction::Delete => {
                return handle_delete_subdir(repo_name, subdir_name, &config);
            }
            SubdirAction::Back => return Ok(()),
        }
    }
}

/// Delete confirmation choice
#[derive(Clone)]
enum DeleteChoice {
    KeepFiles,
    DeleteFiles,
    Cancel,
}

impl FzfSelectable for DeleteChoice {
    fn fzf_display_text(&self) -> String {
        match self {
            DeleteChoice::KeepFiles => format!(
                "{} Keep files (remove from config only)",
                format_icon_colored(NerdFont::File, colors::YELLOW)
            ),
            DeleteChoice::DeleteFiles => format!(
                "{} Delete files from disk",
                format_icon_colored(NerdFont::Trash, colors::RED)
            ),
            DeleteChoice::Cancel => format!("{} Cancel", format_back_icon()),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            DeleteChoice::KeepFiles => "keep".to_string(),
            DeleteChoice::DeleteFiles => "delete".to_string(),
            DeleteChoice::Cancel => "cancel".to_string(),
        }
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        use crate::menu::protocol::FzfPreview;
        match self {
            DeleteChoice::KeepFiles => FzfPreview::Text(
                PreviewBuilder::new()
                    .header(NerdFont::File, "Keep Files")
                    .blank()
                    .text("Remove this directory from the repository config,")
                    .text("but keep the files on disk.")
                    .blank()
                    .text("The directory will no longer be recognized as a")
                    .text("dotfile source, but you can add it back later.")
                    .build_string(),
            ),
            DeleteChoice::DeleteFiles => FzfPreview::Text(
                PreviewBuilder::new()
                    .header(NerdFont::Trash, "Delete Files")
                    .blank()
                    .line(
                        colors::RED,
                        Some(NerdFont::Warning),
                        "This will permanently delete:",
                    )
                    .bullet("The directory and all its contents")
                    .bullet("Any dotfiles stored in this location")
                    .blank()
                    .text("This action cannot be undone!")
                    .build_string(),
            ),
            DeleteChoice::Cancel => FzfPreview::Text(
                PreviewBuilder::new()
                    .header(NerdFont::ArrowLeft, "Cancel")
                    .blank()
                    .text("Go back without making changes.")
                    .build_string(),
            ),
        }
    }
}

/// Handle deleting a subdirectory
fn handle_delete_subdir(repo_name: &str, subdir_name: &str, config: &Config) -> Result<()> {
    // Get the local repo path
    let local_repo = LocalRepo::new(config, repo_name.to_string())?;
    let repo_path = local_repo.local_path(config)?;

    // Check how many subdirs exist
    let meta = crate::dot::meta::read_meta(&repo_path)?;
    if meta.dots_dirs.len() <= 1 {
        FzfWrapper::message(&format!(
            "Cannot delete '{}' - it's the only dotfile directory in this repository.",
            subdir_name
        ))?;
        return Ok(());
    }

    // Show confirmation with options
    let choices = vec![
        DeleteChoice::KeepFiles,
        DeleteChoice::DeleteFiles,
        DeleteChoice::Cancel,
    ];

    let result = FzfWrapper::builder()
        .header(Header::fancy(&format!("Delete '{}'?", subdir_name)))
        .prompt("How do you want to remove this directory?")
        .args(fzf_mocha_args())
        .responsive_layout()
        .select(choices)?;

    match result {
        FzfResult::Selected(DeleteChoice::KeepFiles) => {
            match crate::dot::meta::remove_dots_dir(&repo_path, subdir_name, false) {
                Ok(_) => {
                    // Also remove from active_subdirectories in global config
                    let mut config = Config::load(None)?;
                    if let Some(repo) = config.repos.iter_mut().find(|r| r.name == repo_name) {
                        repo.active_subdirectories.retain(|s| s != subdir_name);
                        config.save(None)?;
                    }
                    FzfWrapper::message(&format!(
                        "Removed '{}' from config. Files kept on disk.",
                        subdir_name
                    ))?;
                }
                Err(e) => {
                    FzfWrapper::message(&format!("Error: {}", e))?;
                }
            }
        }
        FzfResult::Selected(DeleteChoice::DeleteFiles) => {
            match crate::dot::meta::remove_dots_dir(&repo_path, subdir_name, true) {
                Ok(_) => {
                    // Also remove from active_subdirectories in global config
                    let mut config = Config::load(None)?;
                    if let Some(repo) = config.repos.iter_mut().find(|r| r.name == repo_name) {
                        repo.active_subdirectories.retain(|s| s != subdir_name);
                        config.save(None)?;
                    }
                    FzfWrapper::message(&format!(
                        "Deleted '{}' and all its contents.",
                        subdir_name
                    ))?;
                }
                Err(e) => {
                    FzfWrapper::message(&format!("Error: {}", e))?;
                }
            }
        }
        FzfResult::Selected(DeleteChoice::Cancel) | FzfResult::Cancelled => {}
        _ => {}
    }

    Ok(())
}

#[derive(Clone)]
struct SubdirMenuItem {
    subdir: String,
    is_active: bool,
    is_orphaned: bool,
    priority: Option<usize>,
    total_active: usize,
}

impl FzfSelectable for SubdirMenuItem {
    fn fzf_display_text(&self) -> String {
        if self.subdir == ".." {
            format!("{} Back", format_back_icon())
        } else if self.subdir == "__add_new__" {
            format!(
                "{} Add Dotfile Dir",
                format_icon_colored(NerdFont::Plus, colors::GREEN)
            )
        } else if self.is_orphaned {
            // Orphaned: enabled in config but not in metadata
            let mismatch_label = format_icon_colored(NerdFont::Warning, colors::YELLOW);
            format!("{} {} [mismatch]", mismatch_label, self.subdir)
        } else {
            let icon = if self.is_active {
                format_icon_colored(NerdFont::Check, colors::GREEN)
            } else {
                format_icon_colored(NerdFont::CrossCircle, colors::RED)
            };
            // Show priority if active and there are multiple active subdirs
            let priority_text = if let Some(p) = self.priority {
                if self.total_active > 1 {
                    format!(" [P{}]", p)
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            format!("{} {}{}", icon, self.subdir, priority_text)
        }
    }

    fn fzf_key(&self) -> String {
        self.subdir.clone()
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        use crate::menu::protocol::FzfPreview;

        if self.subdir == ".." {
            FzfPreview::Text("Return to repo menu".to_string())
        } else if self.subdir == "__add_new__" {
            FzfPreview::Text(
                PreviewBuilder::new()
                    .header(NerdFont::Plus, "Add Dotfile Directory")
                    .text("Create a new dotfile directory in this repository.")
                    .blank()
                    .text("This will:")
                    .bullet("Create the directory in the repository")
                    .bullet("Add it to instantdots.toml")
                    .bullet("You can then enable it from this menu")
                    .build_string(),
            )
        } else if self.is_orphaned {
            FzfPreview::Text(
                PreviewBuilder::new()
                    .line(
                        colors::YELLOW,
                        Some(NerdFont::Warning),
                        "Configuration Mismatch",
                    )
                    .blank()
                    .text(&format!(
                        "'{}' is enabled in config but not declared",
                        self.subdir
                    ))
                    .text("in the repository's instantdots.toml metadata.")
                    .blank()
                    .text("To resolve this, you can:")
                    .bullet("Disable - Remove from enabled subdirs")
                    .bullet("Add to Metadata - Add to instantdots.toml")
                    .build_string(),
            )
        } else {
            let status = if self.is_active { "Active" } else { "Inactive" };
            let status_color = if self.is_active {
                colors::GREEN
            } else {
                colors::RED
            };
            let mut builder =
                PreviewBuilder::new().line(status_color, None, &format!("Status: {}", status));

            // Add priority info if active
            if let Some(p) = self.priority {
                let priority_hint = if p == 1 && self.total_active > 1 {
                    " (highest priority)"
                } else if p == self.total_active && self.total_active > 1 {
                    " (lowest priority)"
                } else {
                    ""
                };
                builder = builder.line(
                    colors::PEACH,
                    Some(NerdFont::ArrowUp),
                    &format!("Priority: P{}{}", p, priority_hint),
                );
            }

            builder = builder.indented_line(
                colors::TEXT,
                None,
                &format!("Path: {}/dots/{}", self.subdir, self.subdir),
            );

            FzfPreview::Text(builder.build_string())
        }
    }
}

/// Handle managing subdirs
pub fn handle_manage_subdirs(
    repo_name: &str,
    _config: &Config,
    db: &Database,
    debug: bool,
) -> Result<()> {
    loop {
        // Reload config to get current state
        let config = Config::load(None)?;

        // Load the repo to get available subdirs
        let local_repo = match LocalRepo::new(&config, repo_name.to_string()) {
            Ok(repo) => repo,
            Err(e) => {
                FzfWrapper::message(&format!("Failed to load repository: {}", e))?;
                return Ok(());
            }
        };

        let active_subdirs = config.get_active_subdirs(repo_name);

        // Build subdir items with priority info
        let mut subdir_items: Vec<SubdirMenuItem> = local_repo
            .meta
            .dots_dirs
            .iter()
            .map(|subdir| {
                let is_active = active_subdirs.contains(subdir);
                let priority = if is_active {
                    active_subdirs
                        .iter()
                        .position(|s| s == subdir)
                        .map(|i| i + 1)
                } else {
                    None
                };
                SubdirMenuItem {
                    subdir: subdir.clone(),
                    is_active,
                    is_orphaned: false,
                    priority,
                    total_active: active_subdirs.len(),
                }
            })
            .collect();

        // Add "Add Dotfile Dir" option (only for non-read-only repos)
        let repo_config = config.repos.iter().find(|r| r.name == repo_name);
        let is_read_only = repo_config.map(|r| r.read_only).unwrap_or(false);

        if !is_read_only {
            subdir_items.push(SubdirMenuItem {
                subdir: "__add_new__".to_string(),
                is_active: false,
                is_orphaned: false,
                priority: None,
                total_active: 0,
            });
        }

        // Add orphaned subdirs (enabled in config but not in metadata)
        let orphaned = local_repo.get_orphaned_active_subdirs(&config);
        for subdir in orphaned {
            subdir_items.push(SubdirMenuItem {
                subdir,
                is_active: true,
                is_orphaned: true,
                priority: None,
                total_active: 0,
            });
        }

        // Add back option
        subdir_items.push(SubdirMenuItem {
            subdir: "..".to_string(),
            is_active: false,
            is_orphaned: false,
            priority: None,
            total_active: 0,
        });

        let selection = FzfWrapper::builder()
            .header(Header::fancy(&format!("Subdirectories: {}", repo_name)))
            .prompt("Select subdirectory")
            .args(fzf_mocha_args())
            .responsive_layout()
            .select(subdir_items)?;

        let (selected_subdir, is_orphaned) = match selection {
            FzfResult::Selected(item) => (item.subdir, item.is_orphaned),
            FzfResult::Cancelled => return Ok(()),
            _ => return Ok(()),
        };

        if selected_subdir == ".." {
            return Ok(());
        }

        // Handle add new subdirectory
        if selected_subdir == "__add_new__" {
            // Prompt for new directory name
            let new_dir = match FzfWrapper::builder()
                .input()
                .prompt("New dotfile directory name")
                .ghost("e.g. themes, config, scripts")
                .input_result()?
            {
                FzfResult::Selected(s) if !s.trim().is_empty() => s.trim().to_string(),
                FzfResult::Cancelled => continue,
                _ => continue,
            };

            // Get repo path and add the directory
            let local_path = local_repo.local_path(&config)?;
            match crate::dot::meta::add_dots_dir(&local_path, &new_dir) {
                Ok(()) => {
                    FzfWrapper::message(&format!(
                        "Created dotfile directory '{}'. Enable it to start using.",
                        new_dir
                    ))?;
                }
                Err(e) => {
                    FzfWrapper::message(&format!("Error: {}", e))?;
                }
            }
            continue;
        }

        // Handle orphaned subdir with special resolution actions
        if is_orphaned {
            handle_orphaned_subdir_actions(repo_name, &selected_subdir, &local_repo)?;
            continue;
        }

        // Show action menu for the selected subdirectory
        handle_subdir_actions(repo_name, &selected_subdir, &config, db, debug)?;
    }
}

/// Orphaned subdir action
#[derive(Clone)]
enum OrphanedAction {
    Disable,
    AddToMetadata,
    Back,
}

impl FzfSelectable for OrphanedAction {
    fn fzf_display_text(&self) -> String {
        match self {
            OrphanedAction::Disable => format!(
                "{} Disable",
                format_icon_colored(NerdFont::Trash, colors::RED)
            ),
            OrphanedAction::AddToMetadata => format!(
                "{} Add to Metadata",
                format_icon_colored(NerdFont::Plus, colors::GREEN)
            ),
            OrphanedAction::Back => format!("{} Back", format_back_icon()),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            OrphanedAction::Disable => "disable".to_string(),
            OrphanedAction::AddToMetadata => "add".to_string(),
            OrphanedAction::Back => "back".to_string(),
        }
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        use crate::menu::protocol::FzfPreview;
        match self {
            OrphanedAction::Disable => FzfPreview::Text(
                PreviewBuilder::new()
                    .header(NerdFont::Trash, "Disable Subdirectory")
                    .blank()
                    .text("Remove this subdirectory from the enabled list.")
                    .blank()
                    .text("This fixes the mismatch by removing the entry")
                    .text("from your active_subdirectories config.")
                    .build_string(),
            ),
            OrphanedAction::AddToMetadata => FzfPreview::Text(
                PreviewBuilder::new()
                    .header(NerdFont::Plus, "Add to Metadata")
                    .blank()
                    .text("Add this subdirectory to the repository's")
                    .text("instantdots.toml metadata file.")
                    .blank()
                    .text("This fixes the mismatch by declaring the")
                    .text("subdirectory in the repository.")
                    .build_string(),
            ),
            OrphanedAction::Back => FzfPreview::Text("Return to subdirectory list".to_string()),
        }
    }
}

/// Handle orphaned subdir resolution actions
fn handle_orphaned_subdir_actions(
    repo_name: &str,
    subdir_name: &str,
    local_repo: &LocalRepo,
) -> Result<()> {
    let actions = vec![
        OrphanedAction::Disable,
        OrphanedAction::AddToMetadata,
        OrphanedAction::Back,
    ];

    let result = FzfWrapper::builder()
        .header(Header::fancy(&format!("Fix: {} [mismatch]", subdir_name)))
        .prompt("Select action")
        .args(fzf_mocha_args())
        .responsive_layout()
        .select(actions)?;

    let action = match result {
        FzfResult::Selected(item) => item,
        FzfResult::Cancelled => return Ok(()),
        _ => return Ok(()),
    };

    match action {
        OrphanedAction::Disable => {
            let mut config = Config::load(None)?;
            if let Some(repo) = config.repos.iter_mut().find(|r| r.name == repo_name) {
                repo.active_subdirectories.retain(|s| s != subdir_name);
                config.save(None)?;
                FzfWrapper::message(&format!("Disabled '{}'. Mismatch resolved.", subdir_name))?;
            }
        }
        OrphanedAction::AddToMetadata => {
            let repo_path = local_repo.local_path(&Config::load(None)?)?;
            match crate::dot::meta::add_dots_dir(&repo_path, subdir_name) {
                Ok(()) => {
                    FzfWrapper::message(&format!(
                        "Added '{}' to instantdots.toml. Mismatch resolved.",
                        subdir_name
                    ))?;
                }
                Err(e) => {
                    FzfWrapper::message(&format!("Error: {}", e))?;
                }
            }
        }
        OrphanedAction::Back => {}
    }

    Ok(())
}
