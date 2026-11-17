use anyhow::{Context, Result};

use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use crate::dot::dotfilerepo::DotfileRepo;
use crate::dot::repo::DotfileRepositoryManager;
use crate::menu_utils::FzfSelectable;
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::preview::render_repo_preview;

/// Repo action for individual repository menu
#[derive(Debug, Clone, PartialEq, Eq)]
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
    EncryptionSettings,
    Remove,
    Back,
}

#[derive(Clone)]
pub(super) struct RepoActionItem {
    display: String,
    preview: String,
    pub action: RepoAction,
}

impl RepoActionItem {
    fn new(action: RepoAction, label: &str, icon: NerdFont, color: &str, preview: String) -> Self {
        Self {
            display: format!("{} {}", format_icon_colored(icon, color), label),
            preview,
            action,
        }
    }
}

impl FzfSelectable for RepoActionItem {
    fn fzf_display_text(&self) -> String {
        self.display.clone()
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        crate::menu::protocol::FzfPreview::Text(self.preview.clone())
    }
}

/// Build the repo action menu items
pub(super) fn build_repo_action_menu(
    repo_name: &str,
    config: &DotfileConfig,
    db: &Database,
) -> Result<Vec<RepoActionItem>> {
    let (index, repo_config) = config
        .repos
        .iter()
        .enumerate()
        .find(|(_, repo)| repo.name == repo_name)
        .with_context(|| format!("Repository '{repo_name}' not found in configuration"))?;
    let current_position = index + 1;
    let repo_info = DotfileRepositoryManager::new(config, db).get_repository_info(repo_name);

    let mut actions = vec![enabled_action(repo_config.enabled, &repo_info)];
    if current_position > 1 {
        actions.push(priority_action(
            repo_name,
            current_position,
            current_position - 1,
        ));
    }
    if current_position < config.repos.len() {
        actions.push(priority_action(
            repo_name,
            current_position,
            current_position + 1,
        ));
    }

    let is_external = repo_config.is_external();
    let is_read_only = repo_config.read_only;
    let (manage_label, manage_description) = if is_external {
        (
            "Manage Fixed Source",
            "Enable or disable the fixed external repository source rooted at '.'.",
        )
    } else {
        (
            "Manage Subdirs",
            "Enable or disable specific subdirectories within this repository.",
        )
    };

    // Manage active dotfile sources
    actions.push(RepoActionItem::new(
        RepoAction::ManageSubdirs,
        manage_label,
        NerdFont::Folder,
        colors::MAUVE,
        PreviewBuilder::new()
            .line(
                colors::MAUVE,
                Some(NerdFont::Folder),
                &format!("{} for '{}'", manage_label, repo_name),
            )
            .blank()
            .subtext(manage_description)
            .build_string(),
    ));

    // Edit Details (only for writable, non-external repos)
    if !is_read_only && !is_external {
        actions.push(RepoActionItem::new(
            RepoAction::EditDetails,
            "Edit Details",
            NerdFont::Edit,
            colors::BLUE,
            PreviewBuilder::new()
                .line(
                    colors::BLUE,
                    Some(NerdFont::Edit),
                    &format!("Edit '{}' metadata", repo_name),
                )
                .blank()
                .subtext("Edit the author and description in instantdots.toml")
                .build_string(),
        ));
    }

    actions.push(read_only_action(repo_name, is_read_only));

    // Open in Lazygit
    actions.push(RepoActionItem::new(
        RepoAction::OpenInLazygit,
        "Open in Lazygit",
        NerdFont::GitBranch,
        colors::PEACH,
        PreviewBuilder::new()
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
    ));

    // Open in Shell
    actions.push(RepoActionItem::new(
        RepoAction::OpenInShell,
        "Open in Shell",
        NerdFont::Terminal,
        colors::GREEN,
        PreviewBuilder::new()
            .line(
                colors::GREEN,
                Some(NerdFont::Terminal),
                &format!("Open a shell in '{}'", repo_name),
            )
            .blank()
            .subtext("Browse or manually modify files in the repository.")
            .build_string(),
    ));

    // Show info - use the same preview that's shown when the action is selected
    actions.push(RepoActionItem::new(
        RepoAction::ShowInfo,
        "Show Info",
        NerdFont::Info,
        colors::BLUE,
        render_repo_preview(repo_config, current_position, config, &repo_info),
    ));

    // Encryption Settings
    actions.push(RepoActionItem::new(
        RepoAction::EncryptionSettings,
        "Encryption Settings",
        NerdFont::Lock,
        colors::YELLOW,
        PreviewBuilder::new()
            .line(colors::YELLOW, Some(NerdFont::Key), "Encryption Settings")
            .blank()
            .text("Manage encryption recipients and key authorization for this repository.")
            .build_string(),
    ));

    // Remove
    actions.push(RepoActionItem::new(
        RepoAction::Remove,
        "Remove",
        NerdFont::Trash,
        colors::RED,
        PreviewBuilder::new()
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
    ));

    // Back
    actions.push(RepoActionItem {
        display: format!("{} Back", format_back_icon()),
        preview: PreviewBuilder::new()
            .subtext("Return to repository selection")
            .build_string(),
        action: RepoAction::Back,
    });

    Ok(actions)
}

fn enabled_action(is_enabled: bool, repo_info: &Result<DotfileRepo>) -> RepoActionItem {
    let (icon, color, label, next_icon, next_color, instruction, description) = if is_enabled {
        (
            NerdFont::ToggleOn,
            colors::GREEN,
            "Enabled",
            NerdFont::ToggleOff,
            colors::RED,
            "Select to disable",
            "Disabled repositories won't be applied during 'ins dot apply'.",
        )
    } else {
        (
            NerdFont::ToggleOff,
            colors::RED,
            "Disabled",
            NerdFont::ToggleOn,
            colors::GREEN,
            "Select to enable",
            "Enabled repositories will be applied during 'ins dot apply'.",
        )
    };
    let mut preview = PreviewBuilder::new()
        .line(color, Some(icon), &format!("Status: {label}"))
        .blank()
        .line(next_color, Some(next_icon), instruction)
        .blank()
        .subtext(description);

    match repo_info {
        Ok(repo) if !repo.dotfile_dirs.is_empty() => {
            let active = repo.dotfile_dirs.iter().filter(|dir| dir.is_active).count();
            let total = repo.dotfile_dirs.len();
            let label = if is_enabled { "Active" } else { "Selected" };
            preview = preview
                .blank()
                .subtext(&format!("{label} subdirectories: {active}/{total}"));
        }
        Err(error) => {
            preview = preview
                .blank()
                .subtext(&format!("Repository information unavailable: {error:#}"));
        }
        Ok(_) => {}
    }

    RepoActionItem::new(
        RepoAction::Toggle,
        label,
        icon,
        color,
        preview.build_string(),
    )
}

fn priority_action(repo_name: &str, current: usize, next: usize) -> RepoActionItem {
    let (action, label, icon, color, direction, description) = if next < current {
        (
            RepoAction::BumpPriority,
            "Bump Priority",
            NerdFont::ArrowUp,
            colors::PEACH,
            "up",
            "Higher priority repos override lower ones for the same file.",
        )
    } else {
        (
            RepoAction::LowerPriority,
            "Lower Priority",
            NerdFont::ArrowDown,
            colors::LAVENDER,
            "down",
            "Lower priority repos are overridden by higher ones.",
        )
    };
    RepoActionItem::new(
        action,
        label,
        icon,
        color,
        PreviewBuilder::new()
            .line(
                color,
                Some(icon),
                &format!("Move '{repo_name}' {direction} in priority"),
            )
            .blank()
            .field("Current", &format!("P{current}"))
            .field("New", &format!("P{next}"))
            .blank()
            .subtext(description)
            .build_string(),
    )
}

fn read_only_action(repo_name: &str, is_read_only: bool) -> RepoActionItem {
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

    RepoActionItem::new(
        RepoAction::ToggleReadOnly,
        ro_text,
        ro_icon,
        ro_color,
        ro_preview,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::TildePath;
    use crate::dot::config::Repo;
    use crate::dot::types::RepoMetaData;
    use crate::ui::text::strip_ansi;

    fn repo(name: &str) -> Repo {
        Repo {
            name: name.into(),
            url: "https://example.com/dots.git".into(),
            branch: None,
            active_subdirectories: Some(vec!["dots".into()]),
            enabled: true,
            read_only: false,
            metadata: None,
        }
    }

    fn fixture(repos: Vec<Repo>) -> Result<(tempfile::TempDir, DotfileConfig, Database)> {
        let dir = tempfile::tempdir()?;
        let config = DotfileConfig {
            repos,
            repos_dir: TildePath::new(dir.path().join("repos")),
            ..DotfileConfig::default()
        };
        let db = Database::new(dir.path().join("test.db"))?;
        Ok((dir, config, db))
    }

    #[test]
    fn priority_actions_respect_boundaries_and_menu_order() -> Result<()> {
        let (_dir, mut config, db) = fixture(vec![repo("first"), repo("middle"), repo("last")])?;
        for (name, priority_actions) in [
            ("first", vec![RepoAction::LowerPriority]),
            (
                "middle",
                vec![RepoAction::BumpPriority, RepoAction::LowerPriority],
            ),
            ("last", vec![RepoAction::BumpPriority]),
        ] {
            let mut expected = vec![RepoAction::Toggle];
            expected.extend(priority_actions);
            expected.extend([
                RepoAction::ManageSubdirs,
                RepoAction::EditDetails,
                RepoAction::ToggleReadOnly,
                RepoAction::OpenInLazygit,
                RepoAction::OpenInShell,
                RepoAction::ShowInfo,
                RepoAction::EncryptionSettings,
                RepoAction::Remove,
                RepoAction::Back,
            ]);
            let actual: Vec<_> = build_repo_action_menu(name, &config, &db)?
                .into_iter()
                .map(|item| item.action)
                .collect();
            assert_eq!(actual, expected, "{name}");
        }
        config.repos.truncate(1);
        let actions = build_repo_action_menu("first", &config, &db)?;
        assert!(!actions.iter().any(|item| matches!(
            item.action,
            RepoAction::BumpPriority | RepoAction::LowerPriority
        )));
        Ok(())
    }

    #[test]
    fn edit_visibility_and_source_label_follow_repository_kind() -> Result<()> {
        for external in [false, true] {
            for read_only in [false, true] {
                let mut repo = repo("dots");
                repo.read_only = read_only;
                repo.metadata = external.then(RepoMetaData::default);
                let (_dir, config, db) = fixture(vec![repo])?;
                let actions = build_repo_action_menu("dots", &config, &db)?;
                assert_eq!(
                    actions
                        .iter()
                        .any(|item| item.action == RepoAction::EditDetails),
                    !external && !read_only
                );
                let manage = actions
                    .iter()
                    .find(|item| item.action == RepoAction::ManageSubdirs)
                    .context("missing source action")?;
                assert!(manage.display.contains(if external {
                    "Manage Fixed Source"
                } else {
                    "Manage Subdirs"
                }));
                let toggle = actions
                    .iter()
                    .find(|item| item.action == RepoAction::ToggleReadOnly)
                    .context("missing read-only action")?;
                assert!(toggle.display.contains(if read_only {
                    "Make Writable"
                } else {
                    "Make Read-Only"
                }));
            }
        }
        Ok(())
    }

    #[test]
    fn toggle_previews_show_state_and_selected_subdirectory_count() -> Result<()> {
        let mut repo = repo("dots");
        repo.metadata = Some(RepoMetaData {
            dots_dirs: vec!["dots".into(), "extra".into()],
            ..RepoMetaData::default()
        });
        let (_dir, mut config, db) = fixture(vec![repo])?;
        std::fs::create_dir_all(config.repos_path().join("dots"))?;
        for enabled in [true, false] {
            config.repos[0].enabled = enabled;
            let actions = build_repo_action_menu("dots", &config, &db)?;
            let preview = strip_ansi(&actions[0].preview);
            assert!(preview.contains(if enabled {
                "Status: Enabled"
            } else {
                "Status: Disabled"
            }));
            assert!(preview.contains(if enabled {
                "Select to disable"
            } else {
                "Select to enable"
            }));
            assert!(preview.contains(if enabled {
                "Active subdirectories: 1/2"
            } else {
                "Selected subdirectories: 1/2"
            }));
            let info = actions
                .iter()
                .find(|item| item.action == RepoAction::ShowInfo)
                .context("missing info action")?;
            assert_eq!(
                info.preview,
                super::super::preview::build_repo_preview("dots", &config, &db)
            );
        }
        Ok(())
    }

    #[test]
    fn unknown_repository_is_an_error() -> Result<()> {
        let (_dir, config, db) = fixture(vec![repo("dots")])?;
        let error = build_repo_action_menu("missing", &config, &db)
            .err()
            .context("expected missing repository error")?;
        assert!(error.to_string().contains("Repository 'missing' not found"));
        Ok(())
    }

    #[test]
    fn unavailable_repository_information_is_visible_without_hiding_actions() -> Result<()> {
        let (_dir, config, db) = fixture(vec![repo("dots")])?;
        let actions = build_repo_action_menu("dots", &config, &db)?;
        for action in [RepoAction::Toggle, RepoAction::ShowInfo] {
            let item = actions
                .iter()
                .find(|item| item.action == action)
                .context("missing action")?;
            let preview = strip_ansi(&item.preview);
            assert!(preview.contains("Repository information unavailable:"));
            assert!(preview.contains("does not exist"));
        }
        assert!(actions.iter().any(|item| item.action == RepoAction::Remove));
        Ok(())
    }
}
