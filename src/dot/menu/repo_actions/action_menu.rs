use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::repo::RepositoryManager;
use crate::menu_utils::FzfSelectable;
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::preview::build_repo_preview;

/// Repo action for individual repository menu
#[derive(Debug, Clone)]
pub(super) enum RepoAction {
    Toggle,
    BumpPriority,
    LowerPriority,
    ManageSubdirs,
    EditDetails,
    ToggleReadOnly,
    OpenInLazygit,
    OpenInShell,
    ShowInfo,
    Remove,
    Back,
}

#[derive(Clone)]
pub(super) struct RepoActionItem {
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
pub(super) fn build_repo_action_menu(
    repo_name: &str,
    config: &Config,
    db: &Database,
) -> Vec<RepoActionItem> {
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

    // Get repo info for context in toggle preview
    let repo_manager = RepositoryManager::new(config, db);
    let active_subdirs_info =
        repo_manager
            .get_repository_info(repo_name)
            .ok()
            .and_then(|local_repo| {
                let active = local_repo
                    .dotfile_dirs
                    .iter()
                    .filter(|d| d.is_active)
                    .count();
                let total = local_repo.dotfile_dirs.len();
                if total > 0 {
                    Some((active, total))
                } else {
                    None
                }
            });

    let mut actions = Vec::new();

    // Toggle enable/disable (show current state, select to toggle)
    let (icon, color, text, preview) = if is_enabled {
        let mut builder = PreviewBuilder::new()
            .line(colors::GREEN, Some(NerdFont::ToggleOn), "Status: Enabled")
            .blank()
            .line(colors::RED, Some(NerdFont::ToggleOff), "Select to disable")
            .blank()
            .subtext("Disabled repositories won't be applied during 'ins dot apply'.");

        if let Some((active, total)) = active_subdirs_info {
            builder = builder
                .blank()
                .subtext(&format!("Active subdirectories: {active}/{total}"));
        }

        (
            NerdFont::ToggleOn,
            colors::GREEN,
            "Enabled",
            builder.build_string(),
        )
    } else {
        let mut builder = PreviewBuilder::new()
            .line(colors::RED, Some(NerdFont::ToggleOff), "Status: Disabled")
            .blank()
            .line(colors::GREEN, Some(NerdFont::ToggleOn), "Select to enable")
            .blank()
            .subtext("Enabled repositories will be applied during 'ins dot apply'.");

        if let Some((active, total)) = active_subdirs_info {
            builder = builder
                .blank()
                .subtext(&format!("Available subdirectories: {active}/{total}"));
        }

        (
            NerdFont::ToggleOff,
            colors::RED,
            "Disabled",
            builder.build_string(),
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
            preview: PreviewBuilder::new()
                .line(
                    colors::PEACH,
                    Some(NerdFont::ArrowUp),
                    &format!("Move '{}' up in priority", repo_name),
                )
                .blank()
                .field("Current", &format!("P{}", current_position))
                .field("New", &format!("P{}", current_position - 1))
                .blank()
                .subtext("Higher priority repos override lower ones for the same file.")
                .build_string(),
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
            preview: PreviewBuilder::new()
                .line(
                    colors::LAVENDER,
                    Some(NerdFont::ArrowDown),
                    &format!("Move '{}' down in priority", repo_name),
                )
                .blank()
                .field("Current", &format!("P{}", current_position))
                .field("New", &format!("P{}", current_position + 1))
                .blank()
                .subtext("Lower priority repos are overridden by higher ones.")
                .build_string(),
            action: RepoAction::LowerPriority,
        });
    }

    // Manage subdirs
    actions.push(RepoActionItem {
        display: format!(
            "{} Manage Subdirs",
            format_icon_colored(NerdFont::Folder, colors::MAUVE)
        ),
        preview: PreviewBuilder::new()
            .line(
                colors::MAUVE,
                Some(NerdFont::Folder),
                &format!("Manage subdirectories for '{}'", repo_name),
            )
            .blank()
            .subtext("Enable or disable specific subdirectories within this repository.")
            .build_string(),
        action: RepoAction::ManageSubdirs,
    });

    // Edit Details (only for writable, non-external repos)
    let is_read_only = repo_config.map(|r| r.read_only).unwrap_or(false);
    let is_external = repo_config.map(|r| r.metadata.is_some()).unwrap_or(false);

    if !is_read_only && !is_external {
        actions.push(RepoActionItem {
            display: format!(
                "{} Edit Details",
                format_icon_colored(NerdFont::Edit, colors::BLUE)
            ),
            preview: PreviewBuilder::new()
                .line(
                    colors::BLUE,
                    Some(NerdFont::Edit),
                    &format!("Edit '{}' metadata", repo_name),
                )
                .blank()
                .subtext("Edit the author and description in instantdots.toml")
                .build_string(),
            action: RepoAction::EditDetails,
        });
    }

    // Toggle read-only
    let is_read_only = repo_config.map(|r| r.read_only).unwrap_or(false);
    let (ro_icon, ro_color, ro_text, ro_preview) = if is_read_only {
        (
            NerdFont::Lock,
            colors::YELLOW,
            "Make Writable",
            PreviewBuilder::new()
                .line(
                    colors::YELLOW,
                    Some(NerdFont::Unlock),
                    &format!("Make '{}' writable", repo_name),
                )
                .blank()
                .line(colors::RED, Some(NerdFont::Warning), "WARNING")
                .blank()
                .subtext("This will allow the repository to diverge from upstream.")
                .subtext("You may be unable to receive updates without manual work.")
                .blank()
                .separator()
                .blank()
                .subtext("Consider adding your own dotfile repository on top instead.")
                .build_string(),
        )
    } else {
        (
            NerdFont::Lock,
            colors::GREEN,
            "Make Read-Only",
            PreviewBuilder::new()
                .line(
                    colors::GREEN,
                    Some(NerdFont::Lock),
                    &format!("Make '{}' read-only", repo_name),
                )
                .blank()
                .subtext("Read-only repositories cannot be modified by 'ins dot add'.")
                .subtext("This helps keep the repository in sync with upstream.")
                .build_string(),
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
        preview: PreviewBuilder::new()
            .line(
                colors::PEACH,
                Some(NerdFont::GitBranch),
                &format!("Open '{}' in Lazygit", repo_name),
            )
            .blank()
            .text("Lazygit is a terminal UI for git commands.")
            .blank()
            .bullets([
                "View commits",
                "Manage branches",
                "Stage and commit changes",
            ])
            .build_string(),
        action: RepoAction::OpenInLazygit,
    });

    // Open in Shell
    actions.push(RepoActionItem {
        display: format!(
            "{} Open in Shell",
            format_icon_colored(NerdFont::Terminal, colors::GREEN)
        ),
        preview: PreviewBuilder::new()
            .line(
                colors::GREEN,
                Some(NerdFont::Terminal),
                &format!("Open a shell in '{}'", repo_name),
            )
            .blank()
            .subtext("Browse or manually modify files in the repository.")
            .build_string(),
        action: RepoAction::OpenInShell,
    });

    // Show info - use the same preview that's shown when the action is selected
    actions.push(RepoActionItem {
        display: format!(
            "{} Show Info",
            format_icon_colored(NerdFont::Info, colors::BLUE)
        ),
        preview: build_repo_preview(repo_name, config, db),
        action: RepoAction::ShowInfo,
    });

    // Remove
    actions.push(RepoActionItem {
        display: format!(
            "{} Remove",
            format_icon_colored(NerdFont::Trash, colors::RED)
        ),
        preview: PreviewBuilder::new()
            .line(
                colors::RED,
                Some(NerdFont::Trash),
                &format!("Remove '{}'", repo_name),
            )
            .blank()
            .text("Remove this repository from your configuration.")
            .blank()
            .line(
                colors::MAUVE,
                Some(NerdFont::Help),
                "You'll be asked whether to:",
            )
            .bullet("Keep files (just remove from config)")
            .bullet("Delete files (remove from disk too)")
            .build_string(),
        action: RepoAction::Remove,
    });

    // Back
    actions.push(RepoActionItem {
        display: format!("{} Back", format_back_icon()),
        preview: PreviewBuilder::new()
            .subtext("Return to repository selection")
            .build_string(),
        action: RepoAction::Back,
    });

    actions
}
