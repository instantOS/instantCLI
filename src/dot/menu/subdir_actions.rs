//! Subdirectory action handling for the dot menu

use anyhow::Result;
use std::collections::HashSet;

use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::localrepo::LocalRepo;
use crate::dot::meta;
use crate::dot::repo::cli::RepoCommands;
use crate::dot::types::RepoMetaData;
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, Header, MenuCursor};
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
    let configured_subdirs = config
        .repos
        .iter()
        .find(|r| r.name == repo_name)
        .and_then(|repo| repo.active_subdirectories.as_ref());
    let is_active = active_subdirs.contains(&subdir_name.to_string());

    // Find current priority position (1-indexed) only for configured subdirs
    let current_position = configured_subdirs
        .and_then(|subdirs| subdirs.iter().position(|s| s == subdir_name))
        .map(|i| i + 1);
    let total_active = configured_subdirs.map(|subdirs| subdirs.len()).unwrap_or(0);

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

    // Delete (only show if more than one subdir exists AND not an external repo)
    let all_subdirs = config
        .repos
        .iter()
        .find(|r| r.name == repo_name)
        .and_then(|r| r.active_subdirectories.as_ref())
        .map(|subdirs| subdirs.len())
        .unwrap_or(0);

    let is_external = config
        .repos
        .iter()
        .find(|r| r.name == repo_name)
        .map(|r| r.metadata.is_some())
        .unwrap_or(false);

    // Check if this is the only subdir in the repo's instantdots.toml
    // We approximate this by checking if there are multiple subdirs total
    // Skip for external repos (they have a fixed structure)
    if (all_subdirs > 1 || !is_active) && !is_external {
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
    config: &mut Config,
    db: &Database,
    debug: bool,
) -> Result<()> {
    let mut cursor = MenuCursor::new();

    loop {
        let actions = build_subdir_action_menu(repo_name, subdir_name, config);

        let mut builder = FzfWrapper::builder()
            .header(Header::fancy(&format!("{} / {}", repo_name, subdir_name)))
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
            SubdirAction::Toggle => {
                let active_subdirs = config.get_active_subdirs(repo_name);
                let is_active = active_subdirs.contains(&subdir_name.to_string());

                let result = if is_active {
                    let clone_args = RepoCommands::Subdirs {
                        command: crate::dot::repo::cli::SubdirCommands::Disable {
                            name: repo_name.to_string(),
                            subdir: subdir_name.to_string(),
                        },
                    };
                    crate::dot::repo::commands::handle_repo_command(config, db, &clone_args, debug)
                } else {
                    let clone_args = RepoCommands::Subdirs {
                        command: crate::dot::repo::cli::SubdirCommands::Enable {
                            name: repo_name.to_string(),
                            subdir: subdir_name.to_string(),
                        },
                    };
                    crate::dot::repo::commands::handle_repo_command(config, db, &clone_args, debug)
                };

                if let Err(e) = result {
                    FzfWrapper::message(&format!("Error: {}", e))?;
                }
            }
            SubdirAction::BumpPriority => {
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
                return handle_delete_subdir(repo_name, subdir_name, config);
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
fn handle_delete_subdir(repo_name: &str, subdir_name: &str, config: &mut Config) -> Result<()> {
    // Get the local repo path
    let local_repo = LocalRepo::new(config, repo_name.to_string())?;
    let repo_path = local_repo.local_path(config)?;

    // External repos have a fixed structure and cannot have subdirectories removed
    if local_repo.is_external(config) {
        FzfWrapper::message(
            "External repositories use a fixed structure ('.') and cannot have subdirectories added or removed.\n\n\
            To manage subdirectories, convert to a native instantCLI repo by adding an instantdots.toml file.",
        )?;
        return Ok(());
    }

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
                    let mut should_save = false;
                    if let Some(repo) = config.repos.iter_mut().find(|r| r.name == repo_name)
                        && let Some(active_subdirs) = repo.active_subdirectories.as_mut()
                    {
                        active_subdirs.retain(|s| s != subdir_name);
                        should_save = true;
                    }
                    if should_save {
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
                    let mut should_save = false;
                    if let Some(repo) = config.repos.iter_mut().find(|r| r.name == repo_name)
                        && let Some(active_subdirs) = repo.active_subdirectories.as_mut()
                    {
                        active_subdirs.retain(|s| s != subdir_name);
                        should_save = true;
                    }
                    if should_save {
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

const ADD_NEW_SENTINEL: &str = "__add_new__";
const EDIT_DEFAULTS_SENTINEL: &str = "__edit_defaults__";
const BACK_SENTINEL: &str = "..";

#[derive(Clone)]
struct SubdirMenuItem {
    subdir: String,
    is_active: bool,
    is_orphaned: bool,
    priority: Option<usize>,
    total_active: usize,
    default_label: Option<String>,
}

impl FzfSelectable for SubdirMenuItem {
    fn fzf_display_text(&self) -> String {
        if self.subdir == BACK_SENTINEL {
            format!("{} Back", format_back_icon())
        } else if self.subdir == ADD_NEW_SENTINEL {
            format!(
                "{} Add Dotfile Dir",
                format_icon_colored(NerdFont::Plus, colors::GREEN)
            )
        } else if self.subdir == EDIT_DEFAULTS_SENTINEL {
            format!(
                "{} Edit Default Enabled",
                format_icon_colored(NerdFont::Star, colors::YELLOW)
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
            } else if self.is_active {
                " [default]".to_string()
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

        if self.subdir == BACK_SENTINEL {
            FzfPreview::Text("Return to repo menu".to_string())
        } else if self.subdir == ADD_NEW_SENTINEL {
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
        } else if self.subdir == EDIT_DEFAULTS_SENTINEL {
            let current = self
                .default_label
                .as_deref()
                .unwrap_or("Auto (first subdir)");
            FzfPreview::Text(
                PreviewBuilder::new()
                    .header(NerdFont::Star, "Default Enabled")
                    .text("Defaults are only used when you haven't enabled subdirs for this repo.")
                    .blank()
                    .field("Current", current)
                    .blank()
                    .subtext("Select none to disable the repo by default.")
                    .subtext("Select only the first subdir to reset to auto.")
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
            } else if self.is_active {
                builder = builder.line(colors::PEACH, Some(NerdFont::Info), "Default active");
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
    config: &mut Config,
    db: &Database,
    debug: bool,
) -> Result<()> {
    let mut cursor = MenuCursor::new();

    loop {
        // Load the repo to get available subdirs
        let local_repo = match LocalRepo::new(config, repo_name.to_string()) {
            Ok(repo) => repo,
            Err(e) => {
                FzfWrapper::message(&format!("Failed to load repository: {}", e))?;
                return Ok(());
            }
        };

        let active_subdirs = config.get_active_subdirs(repo_name);
        let configured_subdirs = config
            .repos
            .iter()
            .find(|r| r.name == repo_name)
            .and_then(|repo| repo.active_subdirectories.clone());

        // Build subdir items with priority info
        let mut subdir_items: Vec<SubdirMenuItem> = local_repo
            .meta
            .dots_dirs
            .iter()
            .map(|subdir| {
                let is_active = active_subdirs.contains(subdir);
                let is_configured = configured_subdirs
                    .as_ref()
                    .map(|subdirs| subdirs.contains(subdir))
                    .unwrap_or(false);
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
                    total_active: if is_configured {
                        active_subdirs.len()
                    } else {
                        0
                    },
                    default_label: None,
                }
            })
            .collect();

        // Add "Add Dotfile Dir" option (only for non-read-only, non-external repos)
        let repo_config = config.repos.iter().find(|r| r.name == repo_name);
        let is_read_only = repo_config.map(|r| r.read_only).unwrap_or(false);
        let is_external = local_repo.is_external(config);

        if !is_read_only && !is_external && local_repo.meta.dots_dirs.len() > 1 {
            subdir_items.push(SubdirMenuItem {
                subdir: EDIT_DEFAULTS_SENTINEL.to_string(),
                is_active: false,
                is_orphaned: false,
                priority: None,
                total_active: 0,
                default_label: Some(format_default_active_label(&local_repo.meta)),
            });
        }

        if !is_read_only && !is_external {
            subdir_items.push(SubdirMenuItem {
                subdir: ADD_NEW_SENTINEL.to_string(),
                is_active: false,
                is_orphaned: false,
                priority: None,
                total_active: 0,
                default_label: None,
            });
        }

        // Add orphaned subdirs (enabled in config but not in metadata)
        let orphaned = local_repo.get_orphaned_active_subdirs(config);
        for subdir in orphaned {
            subdir_items.push(SubdirMenuItem {
                subdir,
                is_active: true,
                is_orphaned: true,
                priority: None,
                total_active: 0,
                default_label: None,
            });
        }

        // Add back option
        subdir_items.push(SubdirMenuItem {
            subdir: BACK_SENTINEL.to_string(),
            is_active: false,
            is_orphaned: false,
            priority: None,
            total_active: 0,
            default_label: None,
        });

        let defaults_disabled = repo_config
            .map(|repo| repo.active_subdirectories.is_none())
            .unwrap_or(false)
            && local_repo
                .meta
                .default_active_subdirs
                .as_ref()
                .map(|dirs| dirs.is_empty())
                .unwrap_or(false);

        let header_text = if defaults_disabled {
            format!(
                "Subdirectories: {}\nDefaults disabled - repo inactive until you enable subdirs",
                repo_name
            )
        } else {
            format!("Subdirectories: {}", repo_name)
        };

        let mut builder = FzfWrapper::builder()
            .header(Header::fancy(&header_text))
            .prompt("Select subdirectory")
            .args(fzf_mocha_args())
            .responsive_layout();

        if let Some(index) = cursor.initial_index(&subdir_items) {
            builder = builder.initial_index(index);
        }

        let selection = builder.select(subdir_items.clone())?;

        let (selected_subdir, is_orphaned) = match selection {
            FzfResult::Selected(item) => {
                cursor.update(&item, &subdir_items);
                (item.subdir, item.is_orphaned)
            }
            FzfResult::Cancelled => return Ok(()),
            _ => return Ok(()),
        };

        if selected_subdir == BACK_SENTINEL {
            return Ok(());
        }

        if selected_subdir == EDIT_DEFAULTS_SENTINEL {
            handle_edit_default_subdirs(repo_name, &local_repo, config)?;
            continue;
        }

        // Handle add new subdirectory
        if selected_subdir == ADD_NEW_SENTINEL {
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
            let local_path = local_repo.local_path(config)?;
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
            handle_orphaned_subdir_actions(repo_name, &selected_subdir, &local_repo, config)?;
            continue;
        }

        // Show action menu for the selected subdirectory
        handle_subdir_actions(repo_name, &selected_subdir, config, db, debug)?;
    }
}

#[derive(Clone)]
struct DefaultSubdirItem {
    name: String,
    checked: bool,
}

impl FzfSelectable for DefaultSubdirItem {
    fn fzf_display_text(&self) -> String {
        self.name.clone()
    }

    fn fzf_key(&self) -> String {
        self.name.clone()
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        crate::menu::protocol::FzfPreview::Text(
            PreviewBuilder::new()
                .header(NerdFont::Folder, &self.name)
                .text("Include this subdirectory in the default enabled set.")
                .build_string(),
        )
    }

    fn fzf_initial_checked_state(&self) -> bool {
        self.checked
    }
}

fn handle_edit_default_subdirs(
    repo_name: &str,
    local_repo: &LocalRepo,
    config: &Config,
) -> Result<()> {
    if local_repo.is_external(config) {
        FzfWrapper::message(
            "External repositories use metadata from the global config and cannot be edited here.",
        )?;
        return Ok(());
    }

    let repo_config = match config.repos.iter().find(|r| r.name == repo_name) {
        Some(repo) => repo,
        None => {
            FzfWrapper::message(&format!("Repository '{}' not found", repo_name))?;
            return Ok(());
        }
    };

    if repo_config.read_only {
        FzfWrapper::message(&format!(
            "Repository '{}' is read-only. Cannot edit default subdirectories.",
            repo_name
        ))?;
        return Ok(());
    }

    if local_repo.meta.dots_dirs.is_empty() {
        FzfWrapper::message("No dotfile directories are defined for this repository.")?;
        return Ok(());
    }

    let repo_path = local_repo.local_path(config)?;
    let mut metadata = local_repo.meta.clone();
    let current_defaults = resolve_default_active_subdirs(&metadata);
    let current_set: HashSet<String> = current_defaults.into_iter().collect();

    let items: Vec<DefaultSubdirItem> = metadata
        .dots_dirs
        .iter()
        .map(|dir| DefaultSubdirItem {
            name: dir.clone(),
            checked: current_set.contains(dir),
        })
        .collect();

    let selection = FzfWrapper::builder()
        .checklist("Save Defaults")
        .prompt("Toggle defaults")
        .header(Header::fancy(&format!("Default enabled: {}", repo_name)))
        .args(fzf_mocha_args())
        .responsive_layout()
        .checklist_dialog(items)?;

    let selected = match selection {
        FzfResult::MultiSelected(items) => items,
        FzfResult::Cancelled => return Ok(()),
        _ => return Ok(()),
    };

    let selected_names: Vec<String> = selected.into_iter().map(|item| item.name).collect();
    let new_defaults = normalize_default_active_subdirs(&metadata, selected_names);

    if metadata.default_active_subdirs == new_defaults {
        FzfWrapper::message("Default enabled subdirectories unchanged.")?;
        return Ok(());
    }

    metadata.default_active_subdirs = new_defaults.clone();

    match meta::update_meta(&repo_path, &metadata) {
        Ok(()) => match new_defaults {
            Some(defaults) => {
                if defaults.is_empty() {
                    FzfWrapper::message(
                        "Default enabled subdirectories cleared. Repo is disabled until you enable subdirs.",
                    )?;
                } else {
                    FzfWrapper::message(&format!(
                        "Default enabled subdirectories updated: {}",
                        defaults.join(", ")
                    ))?;
                }
            }
            None => {
                let fallback = metadata
                    .dots_dirs
                    .first()
                    .map(|dir| dir.as_str())
                    .unwrap_or("(none)");
                FzfWrapper::message(&format!(
                    "Default enabled subdirectories reset to auto (first: {}).",
                    fallback
                ))?;
            }
        },
        Err(e) => {
            FzfWrapper::message(&format!("Error: {}", e))?;
        }
    }

    Ok(())
}

fn resolve_default_active_subdirs(meta: &RepoMetaData) -> Vec<String> {
    if let Some(defaults) = meta.default_active_subdirs.as_ref() {
        return defaults
            .iter()
            .filter(|dir| meta.dots_dirs.contains(*dir))
            .cloned()
            .collect();
    }

    meta.dots_dirs.first().cloned().into_iter().collect()
}

fn normalize_default_active_subdirs(
    meta: &RepoMetaData,
    selected: Vec<String>,
) -> Option<Vec<String>> {
    let selected_set: HashSet<String> = selected.into_iter().collect();
    let normalized: Vec<String> = meta
        .dots_dirs
        .iter()
        .filter(|dir| selected_set.contains(*dir))
        .cloned()
        .collect();

    let implicit_default = meta.dots_dirs.first().cloned();

    if normalized.is_empty() {
        return Some(Vec::new());
    }

    if let Some(default_dir) = implicit_default
        && normalized.len() == 1
        && normalized[0] == default_dir
    {
        return None;
    }

    Some(normalized)
}

fn format_default_active_label(meta: &RepoMetaData) -> String {
    let defaults = resolve_default_active_subdirs(meta);

    match meta.default_active_subdirs.as_ref() {
        None => {
            let default = defaults.first().map(|s| s.as_str()).unwrap_or("none");
            format!("Auto (first: {})", default)
        }
        Some(dirs) if dirs.is_empty() || defaults.is_empty() => "Disabled (none)".to_string(),
        Some(_) => defaults.join(", "),
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
    config: &mut Config,
) -> Result<()> {
    let actions = vec![
        OrphanedAction::Disable,
        OrphanedAction::AddToMetadata,
        OrphanedAction::Back,
    ];

    let mut cursor = MenuCursor::new();

    let mut builder = FzfWrapper::builder()
        .header(Header::fancy(&format!("Fix: {} [mismatch]", subdir_name)))
        .prompt("Select action")
        .args(fzf_mocha_args())
        .responsive_layout();

    if let Some(index) = cursor.initial_index(&actions) {
        builder = builder.initial_index(index);
    }

    let result = builder.select(actions.clone())?;

    let action = match result {
        FzfResult::Selected(item) => {
            cursor.update(&item, &actions);
            item
        }
        FzfResult::Cancelled => return Ok(()),
        _ => return Ok(()),
    };

    match action {
        OrphanedAction::Disable => {
            let mut should_save = false;
            if let Some(repo) = config.repos.iter_mut().find(|r| r.name == repo_name)
                && let Some(active_subdirs) = repo.active_subdirectories.as_mut()
            {
                active_subdirs.retain(|s| s != subdir_name);
                should_save = true;
            }
            if should_save {
                config.save(None)?;
                FzfWrapper::message(&format!("Disabled '{}'. Mismatch resolved.", subdir_name))?;
            }
        }
        OrphanedAction::AddToMetadata => {
            let repo_path = local_repo.local_path(config)?;
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
