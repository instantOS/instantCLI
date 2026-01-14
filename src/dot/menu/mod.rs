use anyhow::Result;

use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::localrepo::LocalRepo;
use crate::dot::repo::{RepositoryManager, cli::RepoCommands};
use crate::menu_utils::{ConfirmResult, FzfResult, FzfSelectable, FzfWrapper, Header};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

/// Menu entry for dotfile main menu
#[derive(Debug, Clone)]
pub enum DotMenuEntry {
    Repo(String),
    AddRepo,
    CloseMenu,
}

impl FzfSelectable for DotMenuEntry {
    fn fzf_display_text(&self) -> String {
        match self {
            DotMenuEntry::Repo(name) => {
                format!(
                    "{} {}",
                    format_icon_colored(NerdFont::Folder, colors::MAUVE),
                    name
                )
            }
            DotMenuEntry::AddRepo => {
                format!(
                    "{} Add Repo",
                    format_icon_colored(NerdFont::Plus, colors::GREEN)
                )
            }
            DotMenuEntry::CloseMenu => format!("{} Close Menu", format_back_icon()),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            DotMenuEntry::Repo(name) => name.clone(),
            DotMenuEntry::AddRepo => "!__add_repo__".to_string(),
            DotMenuEntry::CloseMenu => "!__close_menu__".to_string(),
        }
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        use crate::menu::protocol::FzfPreview;

        match self {
            DotMenuEntry::Repo(_) => FzfPreview::Text("Repository information".to_string()),
            DotMenuEntry::AddRepo => PreviewBuilder::new()
                .header(NerdFont::Plus, "Add Repository")
                .text("Clone a new dotfile repository")
                .blank()
                .text("This will:")
                .bullet("Prompt for repository URL")
                .bullet("Clone the repository")
                .bullet("Apply dotfiles from the new repo")
                .build(),
            DotMenuEntry::CloseMenu => PreviewBuilder::new()
                .header(NerdFont::Cross, "Close Menu")
                .text("Close the dotfile menu")
                .blank()
                .text("This will exit the interactive menu")
                .text("and return to the command prompt")
                .build(),
        }
    }
}

/// Repo action for individual repository menu
#[derive(Debug, Clone)]
enum RepoAction {
    Toggle,
    BumpPriority,
    LowerPriority,
    ManageSubdirs,
    ShowInfo,
    Remove,
    Back,
}

#[derive(Clone)]
struct RepoActionItem {
    display: String,
    preview: String,
    action: RepoAction,
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
fn build_repo_action_menu(repo_name: &str, config: &Config) -> Vec<RepoActionItem> {
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
            "Remove '{}' from your configuration.\n\nThis will remove the repository from your config. Use --keep-files when removing to preserve local files.",
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
fn build_repo_preview(repo_name: &str, config: &Config, db: &Database) -> String {
    let repo_manager = RepositoryManager::new(config, db);

    let repo_config = match config.repos.iter().find(|r| r.name == repo_name) {
        Some(rc) => rc,
        None => return format!("Repository '{}' not found in config", repo_name),
    };

    let mut builder = PreviewBuilder::new()
        .title(colors::SKY, repo_name)
        .blank()
        .line(
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

    if priority > 0 {
        builder = builder.line(
            colors::PEACH,
            Some(NerdFont::ArrowUp),
            &format!("Priority: P{}", priority),
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
            let active = if repo_config.active_subdirectories.is_empty() {
                "dots".to_string()
            } else {
                repo_config.active_subdirectories.join(", ")
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

/// Select a menu entry from the main dot menu
fn select_dot_menu_entry(config: &Config, db: &Database) -> Result<Option<DotMenuEntry>> {
    let mut entries: Vec<DotMenuEntry> = config
        .repos
        .iter()
        .map(|r| DotMenuEntry::Repo(r.name.clone()))
        .collect();

    entries.push(DotMenuEntry::AddRepo);
    entries.push(DotMenuEntry::CloseMenu);

    // Create entries with custom previews
    let menu_items: Vec<DotMenuItem> = entries
        .into_iter()
        .map(|entry| DotMenuItem {
            entry: entry.clone(),
            preview: match &entry {
                DotMenuEntry::Repo(name) => build_repo_preview(name, config, db),
                DotMenuEntry::AddRepo => PreviewBuilder::new()
                    .header(NerdFont::Plus, "Add Repository")
                    .text("Clone a new dotfile repository")
                    .blank()
                    .text("This will:")
                    .bullet("Prompt for repository URL")
                    .bullet("Clone the repository")
                    .bullet("Apply dotfiles from the new repo")
                    .build_string(),
                DotMenuEntry::CloseMenu => PreviewBuilder::new()
                    .header(NerdFont::Cross, "Close Menu")
                    .text("Close the dotfile menu")
                    .blank()
                    .text("This will exit the interactive menu")
                    .text("and return to the command prompt")
                    .build_string(),
            },
        })
        .collect();

    let result = FzfWrapper::builder()
        .header(Header::fancy("Dotfile Menu"))
        .prompt("Select")
        .args(fzf_mocha_args())
        .responsive_layout()
        .select(menu_items)?;

    match result {
        FzfResult::Selected(item) => Ok(Some(item.entry)),
        FzfResult::Cancelled => Ok(None),
        _ => Ok(None),
    }
}

/// Wrapper struct for menu items with custom previews
#[derive(Clone)]
struct DotMenuItem {
    entry: DotMenuEntry,
    preview: String,
}

impl FzfSelectable for DotMenuItem {
    fn fzf_display_text(&self) -> String {
        self.entry.fzf_display_text()
    }

    fn fzf_key(&self) -> String {
        self.entry.fzf_key()
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        crate::menu::protocol::FzfPreview::Text(self.preview.clone())
    }
}

/// Handle adding a new repository
fn handle_add_repo(config: &Config, db: &Database, debug: bool) -> Result<()> {
    use crate::menu_utils::FzfResult;

    const DEFAULT_REPO: &str = "https://github.com/instantOS/dotfiles";

    // Loop for URL input (allows "enter another URL" option)
    let url = loop {
        // Prompt for URL with example ghost text
        match FzfWrapper::builder()
            .input()
            .prompt("Repository URL")
            .ghost(DEFAULT_REPO)
            .input_result()?
        {
            FzfResult::Selected(s) if !s.is_empty() => break s,
            FzfResult::Cancelled => return Ok(()),
            FzfResult::Selected(_) => {
                // Empty input - show choice menu
                #[derive(Clone)]
                enum EmptyUrlChoice {
                    UseDefault,
                    GoBack,
                    EnterAnother,
                }

                impl crate::menu_utils::FzfSelectable for EmptyUrlChoice {
                    fn fzf_display_text(&self) -> String {
                        match self {
                            EmptyUrlChoice::UseDefault => format!(
                                "{} Use default (instantOS/dotfiles)",
                                format_icon_colored(NerdFont::Check, colors::GREEN)
                            ),
                            EmptyUrlChoice::GoBack => format!("{} Go back", format_back_icon()),
                            EmptyUrlChoice::EnterAnother => format!(
                                "{} Enter another URL",
                                format_icon_colored(NerdFont::Edit, colors::BLUE)
                            ),
                        }
                    }
                    fn fzf_key(&self) -> String {
                        match self {
                            EmptyUrlChoice::UseDefault => "default".to_string(),
                            EmptyUrlChoice::GoBack => "back".to_string(),
                            EmptyUrlChoice::EnterAnother => "another".to_string(),
                        }
                    }
                    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
                        crate::menu::protocol::FzfPreview::None
                    }
                }

                let choices = vec![
                    EmptyUrlChoice::UseDefault,
                    EmptyUrlChoice::GoBack,
                    EmptyUrlChoice::EnterAnother,
                ];

                match FzfWrapper::builder()
                    .header(Header::fancy("No URL entered"))
                    .prompt("Select")
                    .args(fzf_mocha_args())
                    .select(choices)?
                {
                    FzfResult::Selected(EmptyUrlChoice::UseDefault) => {
                        break DEFAULT_REPO.to_string();
                    }
                    FzfResult::Selected(EmptyUrlChoice::GoBack) | FzfResult::Cancelled => {
                        return Ok(());
                    }
                    FzfResult::Selected(EmptyUrlChoice::EnterAnother) => continue,
                    _ => return Ok(()),
                }
            }
            _ => return Ok(()),
        }
    };

    // Optionally prompt for name
    let name = match FzfWrapper::builder()
        .input()
        .prompt("Repository name (optional)")
        .input_result()?
    {
        FzfResult::Selected(s) if !s.is_empty() => Some(s),
        FzfResult::Selected(_) => None,
        FzfResult::Cancelled => return Ok(()),
        _ => None,
    };

    // Optionally prompt for branch
    let branch = match FzfWrapper::builder()
        .input()
        .prompt("Branch (optional)")
        .input_result()?
    {
        FzfResult::Selected(s) if !s.is_empty() => Some(s),
        FzfResult::Selected(_) => None,
        FzfResult::Cancelled => return Ok(()),
        _ => None,
    };

    // Use the existing clone command
    let clone_args = RepoCommands::Clone(crate::dot::repo::cli::CloneArgs {
        url,
        name,
        branch,
        read_only: false,
        force_write: false,
    });

    // We need a mutable config for this, but we have an immutable reference
    // So we reload and re-save
    let mut config = Config::load(None)?;
    let db = Database::new(config.database_path().to_path_buf())?;

    crate::dot::repo::commands::handle_repo_command(&mut config, &db, &clone_args, debug)?;

    Ok(())
}

/// Handle repo actions
fn handle_repo_actions(repo_name: &str, config: &Config, db: &Database, debug: bool) -> Result<()> {
    loop {
        let actions = build_repo_action_menu(repo_name, config);

        let result = FzfWrapper::builder()
            .header(Header::fancy(&format!("Repository: {}", repo_name)))
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
                handle_manage_subdirs(repo_name, config, db, debug)?;
            }
            RepoAction::ShowInfo => {
                // Build the info string using the preview builder
                let config = Config::load(None)?;
                let db = Database::new(config.database_path().to_path_buf())?;
                let info_text = build_repo_preview(repo_name, &config, &db);

                // Display in a message dialog
                FzfWrapper::builder()
                    .message(&info_text)
                    .title(repo_name)
                    .message_dialog()?;
            }
            RepoAction::Remove => {
                let confirm = FzfWrapper::builder()
                    .confirm(&format!(
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
        }
    }
}

/// Handle managing subdirs
fn handle_manage_subdirs(
    repo_name: &str,
    config: &Config,
    _db: &Database,
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

        // Build subdir items
        let mut subdir_items: Vec<SubdirMenuItem> = local_repo
            .meta
            .dots_dirs
            .iter()
            .map(|subdir| {
                let is_active = active_subdirs.contains(subdir);
                SubdirMenuItem {
                    subdir: subdir.clone(),
                    is_active,
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
            });
        }

        // Add back option
        subdir_items.push(SubdirMenuItem {
            subdir: "..".to_string(),
            is_active: false,
        });

        let selection = FzfWrapper::builder()
            .header(Header::fancy(&format!("Subdirectories: {}", repo_name)))
            .prompt("Select subdirectory")
            .args(fzf_mocha_args())
            .responsive_layout()
            .select(subdir_items)?;

        let selected_subdir = match selection {
            FzfResult::Selected(item) => item.subdir,
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

        // Determine current state and toggle
        let is_active = active_subdirs.contains(&selected_subdir);

        let mut config = Config::load(None)?;
        let db = Database::new(config.database_path().to_path_buf())?;

        let result = if is_active {
            let clone_args = RepoCommands::Subdirs {
                command: crate::dot::repo::cli::SubdirCommands::Disable {
                    name: repo_name.to_string(),
                    subdir: selected_subdir.clone(),
                },
            };
            crate::dot::repo::commands::handle_repo_command(&mut config, &db, &clone_args, debug)
        } else {
            let clone_args = RepoCommands::Subdirs {
                command: crate::dot::repo::cli::SubdirCommands::Enable {
                    name: repo_name.to_string(),
                    subdir: selected_subdir.clone(),
                },
            };
            crate::dot::repo::commands::handle_repo_command(&mut config, &db, &clone_args, debug)
        };

        // Handle errors gracefully - show message and continue menu loop
        if let Err(e) = result {
            FzfWrapper::message(&format!("Error: {}", e))?;
        }

        // Loop continues to show updated list
    }
}

#[derive(Clone)]
struct SubdirMenuItem {
    subdir: String,
    is_active: bool,
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
        } else {
            let icon = if self.is_active {
                format_icon_colored(NerdFont::Check, colors::GREEN)
            } else {
                format_icon_colored(NerdFont::CrossCircle, colors::RED)
            };
            format!("{} {}", icon, self.subdir)
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
        } else {
            let status = if self.is_active { "Active" } else { "Inactive" };
            let status_color = if self.is_active {
                colors::GREEN
            } else {
                colors::RED
            };
            FzfPreview::Text(
                PreviewBuilder::new()
                    .line(status_color, None, &format!("Status: {}", status))
                    .indented_line(
                        colors::TEXT,
                        None,
                        &format!("Path: {}/dots/{}", self.subdir, self.subdir),
                    )
                    .build_string(),
            )
        }
    }
}

/// Main entry point for the dot menu
pub fn dot_menu(config: &Config, db: &Database, debug: bool) -> Result<()> {
    // Outer loop: main menu
    loop {
        let entry = match select_dot_menu_entry(config, db)? {
            Some(entry) => entry,
            None => return Ok(()),
        };

        match entry {
            DotMenuEntry::Repo(repo_name) => {
                handle_repo_actions(&repo_name, config, db, debug)?;
            }
            DotMenuEntry::AddRepo => {
                handle_add_repo(config, db, debug)?;
            }
            DotMenuEntry::CloseMenu => return Ok(()),
        }
    }
}
