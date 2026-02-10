//! Subdir action menu for a selected subdirectory.

use anyhow::Result;

use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use crate::dot::repo::cli::RepoCommands;
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, Header, MenuCursor};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::delete::handle_delete_subdir;

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

pub(crate) fn handle_subdir_actions(
    repo_name: &str,
    subdir_name: &str,
    config: &mut DotfileConfig,
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
                handle_toggle_action(repo_name, subdir_name, config, db, debug)?
            }
            SubdirAction::BumpPriority => {
                handle_priority_move(repo_name, subdir_name, config, PriorityDirection::Up)?;
            }
            SubdirAction::LowerPriority => {
                handle_priority_move(repo_name, subdir_name, config, PriorityDirection::Down)?;
            }
            SubdirAction::Delete => {
                return handle_delete_subdir(repo_name, subdir_name, config);
            }
            SubdirAction::Back => return Ok(()),
        }
    }
}

/// Build the subdir action menu items
fn build_subdir_action_menu(
    repo_name: &str,
    subdir_name: &str,
    config: &DotfileConfig,
) -> Vec<SubdirActionItem> {
    let active_subdirs = config.get_active_subdirs(repo_name);
    let repo_config = config.repos.iter().find(|r| r.name == repo_name);
    let configured_subdirs = repo_config.and_then(|repo| repo.active_subdirectories.as_ref());
    let is_active = active_subdirs.iter().any(|s| s == subdir_name);

    // Find current priority position (1-indexed) only for configured subdirs
    let current_position = configured_subdirs
        .and_then(|subdirs| subdirs.iter().position(|s| s == subdir_name))
        .map(|i| i + 1);
    let total_active = configured_subdirs.map(|subdirs| subdirs.len()).unwrap_or(0);

    let mut actions = Vec::new();

    // Toggle enable/disable (show current state, select to toggle)
    let (icon, color, text, preview) = if is_active {
        (
            NerdFont::ToggleOn,
            colors::GREEN,
            "Enabled",
            PreviewBuilder::new()
                .line(colors::GREEN, Some(NerdFont::ToggleOn), "Status: Enabled")
                .blank()
                .line(colors::RED, Some(NerdFont::ToggleOff), "Select to disable")
                .blank()
                .subtext("Disabled subdirectories won't be applied during 'ins dot apply'.")
                .build_string(),
        )
    } else {
        (
            NerdFont::ToggleOff,
            colors::RED,
            "Disabled",
            PreviewBuilder::new()
                .line(colors::RED, Some(NerdFont::ToggleOff), "Status: Disabled")
                .blank()
                .line(colors::GREEN, Some(NerdFont::ToggleOn), "Select to enable")
                .blank()
                .subtext("Enabled subdirectories will be applied during 'ins dot apply'.")
                .build_string(),
        )
    };

    actions.push(action_item(
        icon,
        color,
        text,
        preview,
        SubdirAction::Toggle,
    ));

    // Priority options only for active subdirs with more than one active
    if let Some(pos) = current_position {
        // Priority: Bump up (only if not already at top and more than one active)
        if pos > 1 && total_active > 1 {
            actions.push(action_item(
                NerdFont::ArrowUp,
                colors::PEACH,
                "Bump Priority",
                format!(
                    "Move '{}' up in priority.\n\nCurrent: P{} → New: P{}\n\nHigher priority subdirs override lower ones for the same file.",
                    subdir_name,
                    pos,
                    pos - 1
                ),
                SubdirAction::BumpPriority,
            ));
        }

        // Priority: Lower down (only if not already at bottom and more than one active)
        if pos < total_active && total_active > 1 {
            actions.push(action_item(
                NerdFont::ArrowDown,
                colors::LAVENDER,
                "Lower Priority",
                format!(
                    "Move '{}' down in priority.\n\nCurrent: P{} → New: P{}\n\nHigher priority subdirs override lower ones for the same file.",
                    subdir_name,
                    pos,
                    pos + 1
                ),
                SubdirAction::LowerPriority,
            ));
        }
    }

    // Delete (only show if more than one subdir exists AND not an external repo)
    let all_subdirs = repo_config
        .and_then(|repo| repo.active_subdirectories.as_ref())
        .map(|subdirs| subdirs.len())
        .unwrap_or(0);

    let is_external = repo_config
        .map(|repo| repo.metadata.is_some())
        .unwrap_or(false);

    // Check if this is the only subdir in the repo's instantdots.toml
    // We approximate this by checking if there are multiple subdirs total
    // Skip for external repos (they have a fixed structure)
    if (all_subdirs > 1 || !is_active) && !is_external {
        actions.push(action_item(
            NerdFont::Trash,
            colors::RED,
            "Delete",
            format!(
                "Remove '{}' from this repository.\n\n\
                You'll be asked whether to:\n\
                • Keep files (just remove from config)\n\
                • Delete files (remove from disk too)",
                subdir_name
            ),
            SubdirAction::Delete,
        ));
    }

    // Back
    actions.push(SubdirActionItem {
        display: format!("{} Back", format_back_icon()),
        preview: "Return to subdirectory selection".to_string(),
        action: SubdirAction::Back,
    });

    actions
}

fn action_item(
    icon: NerdFont,
    color: &'static str,
    text: &str,
    preview: String,
    action: SubdirAction,
) -> SubdirActionItem {
    SubdirActionItem {
        display: format!("{} {}", format_icon_colored(icon, color), text),
        preview,
        action,
    }
}

fn handle_toggle_action(
    repo_name: &str,
    subdir_name: &str,
    config: &mut DotfileConfig,
    db: &Database,
    debug: bool,
) -> Result<()> {
    let is_active = config
        .get_active_subdirs(repo_name)
        .iter()
        .any(|s| s == subdir_name);

    let command = if is_active {
        RepoCommands::Subdirs {
            command: crate::dot::repo::cli::SubdirCommands::Disable {
                name: repo_name.to_string(),
                subdir: subdir_name.to_string(),
            },
        }
    } else {
        RepoCommands::Subdirs {
            command: crate::dot::repo::cli::SubdirCommands::Enable {
                name: repo_name.to_string(),
                subdir: subdir_name.to_string(),
            },
        }
    };

    if let Err(e) = crate::dot::repo::commands::handle_repo_command(config, db, &command, debug) {
        FzfWrapper::message(&format!("Error: {}", e))?;
    }

    Ok(())
}

enum PriorityDirection {
    Up,
    Down,
}

fn handle_priority_move(
    repo_name: &str,
    subdir_name: &str,
    config: &mut DotfileConfig,
    direction: PriorityDirection,
) -> Result<()> {
    let result = match direction {
        PriorityDirection::Up => config.move_subdir_up(repo_name, subdir_name, None),
        PriorityDirection::Down => config.move_subdir_down(repo_name, subdir_name, None),
    };

    match result {
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

    Ok(())
}
