//! Alternative source selection for dotfiles.
//!
//! Allows users to select which repository/subdirectory a dotfile is sourced from.

mod apply;
mod discovery;
mod picker;

use std::path::Path;

use anyhow::Result;
use colored::Colorize;

use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::override_config::{DotfileSource, OverrideConfig, find_all_sources};
use crate::dot::utils::resolve_dotfile_path;
use crate::menu_utils::{FzfResult, FzfWrapper};
use crate::ui::catppuccin::fzf_mocha_args;
use crate::ui::prelude::*;

use apply::{is_safe_to_switch, remove_override, reset_override, set_alternative};
use discovery::{DiscoveryFilter, discover_dotfiles, get_destinations, to_display_path};
use picker::{BrowseMenuItem, CreateMenuItem, MenuItem, SourceOption};

// Re-export for external use (add command uses these)
pub use apply::add_to_destination;

/// Pick a destination and add a file there (shared by `add --choose` and `alternative --create`).
///
/// Shows an FZF picker with all available destinations plus options to:
/// - Add a new dotfile subdir to an existing repo
/// - Clone a new repository
///
/// Returns Ok(true) if the file was added, Ok(false) if cancelled.
pub fn pick_destination_and_add(config: &Config, path: &Path) -> Result<bool> {
    let display = to_display_path(path);
    let existing = find_all_sources(config, path)?;
    create_flow(config, path, &display, &existing).map(|()| true)
}

/// CLI action for the alternative command.
pub enum Action {
    /// Interactive selection (default).
    Select,
    /// Create alternative in new location.
    Create,
    /// List available alternatives.
    List,
    /// Reset override to default.
    Reset,
}

impl Action {
    pub fn from_flags(reset: bool, create: bool, list: bool) -> Self {
        if reset {
            Self::Reset
        } else if create {
            Self::Create
        } else if list {
            Self::List
        } else {
            Self::Select
        }
    }
}

/// Main entry point for the alternative command.
pub fn handle_alternative(
    config: &Config,
    path: &str,
    reset: bool,
    create: bool,
    list: bool,
) -> Result<()> {
    let action = Action::from_flags(reset, create, list);
    let target_path = resolve_dotfile_path(path)?;
    let display_path = to_display_path(&target_path);

    if target_path.is_dir() {
        return handle_directory(config, &target_path, &display_path, action);
    }

    handle_file(config, &target_path, &display_path, action)
}

// ─────────────────────────────────────────────────────────────────────────────
// Directory handlers
// ─────────────────────────────────────────────────────────────────────────────

fn handle_directory(config: &Config, dir: &Path, display: &str, action: Action) -> Result<()> {
    match action {
        Action::Reset => Err(anyhow::anyhow!(
            "--reset is not supported for directories. Use it with a specific file."
        )),
        Action::List => list_directory(config, dir, display),
        Action::Select => browse_directory(config, dir, display, false),
        Action::Create => browse_directory(config, dir, display, true),
    }
}

fn browse_directory(config: &Config, dir: &Path, display: &str, create_mode: bool) -> Result<()> {
    let filter = if create_mode {
        DiscoveryFilter::All
    } else {
        DiscoveryFilter::WithAlternatives
    };

    // Check once at the start if there are any dotfiles
    let initial_dotfiles = discover_dotfiles(config, dir, filter)?;
    if initial_dotfiles.is_empty() {
        if create_mode {
            emit(
                Level::Info,
                "dot.alternative.empty",
                &format!("No dotfiles found in {}", display.cyan()),
                None,
            );
            return Ok(());
        }
        // No alternatives found - offer to create one
        return offer_create_alternative(config, dir, display);
    }

    loop {
        // Reload config and rediscover dotfiles each iteration to pick up newly tracked files
        let config = Config::load(None)?;
        let dotfiles = discover_dotfiles(&config, dir, filter)?;

        let action = if create_mode {
            "create alternative"
        } else {
            "switch source"
        };
        emit(
            Level::Info,
            "dot.alternative.found",
            &format!(
                "{} Found {} dotfiles in {} (select to {})",
                char::from(NerdFont::Check),
                dotfiles.len(),
                display.cyan(),
                action
            ),
            None,
        );

        // Build menu items - actions at start so they appear at top in FZF
        let mut menu: Vec<BrowseMenuItem> = Vec::new();

        if create_mode {
            menu.push(BrowseMenuItem::PickNewFile);
        }
        menu.push(BrowseMenuItem::Cancel);

        // Add dotfiles after the action items
        menu.extend(dotfiles.into_iter().map(BrowseMenuItem::Dotfile));

        let selection = FzfWrapper::builder()
            .prompt(format!("Select dotfile in {}: ", display))
            .args(fzf_mocha_args())
            .responsive_layout()
            .select(menu)?;

        match selection {
            FzfResult::Selected(BrowseMenuItem::Dotfile(selected)) => {
                if create_mode {
                    let sources = find_all_sources(&config, &selected.target_path)?;
                    create_flow(
                        &config,
                        &selected.target_path,
                        &selected.display_path,
                        &sources,
                    )?;
                    // Loop back to show updated menu with newly tracked file
                    continue;
                } else {
                    return select_flow(
                        &config,
                        &selected.target_path,
                        &selected.display_path,
                        true,
                    );
                }
            }
            FzfResult::Selected(BrowseMenuItem::PickNewFile) => {
                if let Some(path) = pick_new_file_to_track()? {
                    let display_path = to_display_path(&path);
                    let sources = find_all_sources(&config, &path)?;
                    create_flow(&config, &path, &display_path, &sources)?;
                    // Loop back to show updated menu with newly tracked file
                }
                continue;
            }
            FzfResult::Selected(BrowseMenuItem::Cancel) | FzfResult::Cancelled => {
                emit_cancelled();
                return Ok(());
            }
            FzfResult::Error(e) => return Err(anyhow::anyhow!("Selection error: {}", e)),
            _ => return Ok(()),
        }
    }
}

/// Offer to create a new alternative when none exist
fn offer_create_alternative(config: &Config, dir: &Path, display: &str) -> Result<()> {
    use crate::menu_utils::Header;

    #[derive(Clone)]
    enum Choice {
        CreateAlternative,
        Cancel,
    }

    impl crate::menu_utils::FzfSelectable for Choice {
        fn fzf_display_text(&self) -> String {
            match self {
                Choice::CreateAlternative => format!(
                    "{} Create new alternative...",
                    crate::ui::catppuccin::format_icon_colored(
                        NerdFont::Plus,
                        crate::ui::catppuccin::colors::GREEN
                    )
                ),
                Choice::Cancel => format!(
                    "{} Cancel",
                    crate::ui::catppuccin::format_icon_colored(
                        NerdFont::Cross,
                        crate::ui::catppuccin::colors::OVERLAY0
                    )
                ),
            }
        }

        fn fzf_key(&self) -> String {
            match self {
                Choice::CreateAlternative => "create".to_string(),
                Choice::Cancel => "cancel".to_string(),
            }
        }

        fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
            use crate::ui::preview::PreviewBuilder;
            match self {
                Choice::CreateAlternative => crate::menu::protocol::FzfPreview::Text(
                    PreviewBuilder::new()
                        .header(NerdFont::Plus, "Create Alternative")
                        .blank()
                        .text("Add a dotfile to a new repository location.")
                        .blank()
                        .text("This lets you:")
                        .bullet("Copy a dotfile to another repo")
                        .bullet("Create themed variations")
                        .bullet("Set up machine-specific configs")
                        .build_string(),
                ),
                Choice::Cancel => crate::menu::protocol::FzfPreview::Text(
                    PreviewBuilder::new()
                        .header(NerdFont::Cross, "Cancel")
                        .blank()
                        .text("Exit without making changes.")
                        .build_string(),
                ),
            }
        }
    }

    emit(
        Level::Info,
        "dot.alternative.none_found",
        &format!(
            "{} No dotfiles with alternatives in {}",
            char::from(NerdFont::Info),
            display.cyan()
        ),
        None,
    );

    let choices = vec![Choice::CreateAlternative, Choice::Cancel];

    match FzfWrapper::builder()
        .header(Header::fancy("No alternatives found"))
        .prompt("Select action: ")
        .args(fzf_mocha_args())
        .responsive_layout()
        .select(choices)?
    {
        FzfResult::Selected(Choice::CreateAlternative) => {
            // Switch to create mode - show all dotfiles
            browse_directory(config, dir, display, true)
        }
        FzfResult::Selected(Choice::Cancel) | FzfResult::Cancelled => {
            emit_cancelled();
            Ok(())
        }
        _ => Ok(()),
    }
}

fn list_directory(config: &Config, dir: &Path, display: &str) -> Result<()> {
    let dotfiles = discover_dotfiles(config, dir, DiscoveryFilter::WithAlternatives)?;

    if dotfiles.is_empty() {
        emit(
            Level::Info,
            "dot.alternative.list.empty",
            &format!("No dotfiles with alternatives in {}", display.cyan()),
            None,
        );
        return Ok(());
    }

    let overrides = OverrideConfig::load()?;
    emit(
        Level::Info,
        "dot.alternative.list.header",
        &format!(
            "{} Alternatives for {} dotfiles in {}:",
            char::from(NerdFont::List),
            dotfiles.len(),
            display.cyan()
        ),
        None,
    );

    for dotfile in &dotfiles {
        print_sources(
            &dotfile.target_path,
            &dotfile.display_path,
            &dotfile.sources,
            &overrides,
        );
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// File handlers
// ─────────────────────────────────────────────────────────────────────────────

fn handle_file(config: &Config, path: &Path, display: &str, action: Action) -> Result<()> {
    match action {
        Action::Reset => reset_override(path, display),
        Action::List => {
            let sources = find_all_sources(config, path)?;
            list_file(path, display, &sources)
        }
        Action::Create => {
            let sources = find_all_sources(config, path)?;
            create_flow(config, path, display, &sources)
        }
        Action::Select => select_flow(config, path, display, false),
    }
}

fn select_flow(config: &Config, path: &Path, display: &str, from_menu: bool) -> Result<()> {
    let sources = find_all_sources(config, path)?;

    if sources.is_empty() {
        emit(
            Level::Warn,
            "dot.alternative.not_found",
            &format!(
                "{} No sources found for {}. Use --create to add it.",
                char::from(NerdFont::Warning),
                display.yellow()
            ),
            None,
        );
        return Ok(());
    }

    if sources.len() == 1 {
        return show_single_source(config, display, &sources[0]);
    }

    let overrides = OverrideConfig::load()?;
    let current = overrides.get_override(path);
    let default_source = sources.last().cloned();

    let items: Vec<SourceOption> = sources
        .into_iter()
        .map(|source| {
            let is_current = current
                .map(|o| o.source_repo == source.repo_name && o.source_subdir == source.subdir_name)
                .unwrap_or(false);
            SourceOption {
                source,
                is_current,
                exists: true,
            }
        })
        .collect();

    if !is_safe_to_switch(path, &items)? {
        emit(
            Level::Error,
            "dot.alternative.modified",
            &format!(
                "{} Cannot switch {} - file modified. Use 'ins dot reset {}' first.",
                char::from(NerdFont::CrossCircle),
                display.yellow(),
                display
            ),
            None,
        );
        return Ok(());
    }

    let mut menu: Vec<MenuItem> = items.into_iter().map(MenuItem::Source).collect();

    if current.is_some()
        && let Some(default) = default_source
    {
        menu.push(MenuItem::RemoveOverride {
            default_source: default,
        });
    }

    menu.push(if from_menu {
        MenuItem::Back
    } else {
        MenuItem::Cancel
    });

    match FzfWrapper::builder()
        .prompt(format!("Select source for {}: ", display))
        .args(fzf_mocha_args())
        .responsive_layout()
        .select(menu)?
    {
        FzfResult::Selected(MenuItem::Source(item)) => {
            set_alternative(config, path, display, &item)
        }
        FzfResult::Selected(MenuItem::RemoveOverride { default_source }) => {
            remove_override(config, path, display, &default_source)
        }
        FzfResult::Selected(MenuItem::Back | MenuItem::Cancel) => Ok(()),
        FzfResult::Cancelled => {
            emit_cancelled();
            Ok(())
        }
        FzfResult::Error(e) => Err(anyhow::anyhow!("Selection error: {}", e)),
        _ => Ok(()),
    }
}

fn create_flow(
    _config: &Config,
    path: &Path,
    display: &str,
    existing: &[DotfileSource],
) -> Result<()> {
    use std::collections::HashSet;

    loop {
        // Reload config to pick up any new repos/subdirs
        let config = Config::load(None)?;
        let destinations = get_destinations(&config);

        // Build menu items
        let mut menu: Vec<CreateMenuItem> = destinations
            .iter()
            .map(|dest| {
                let exists = existing
                    .iter()
                    .any(|s| s.repo_name == dest.repo_name && s.subdir_name == dest.subdir_name);
                CreateMenuItem::Destination(SourceOption {
                    source: dest.clone(),
                    is_current: false,
                    exists,
                })
            })
            .collect();

        // Add "new subdir" option for each writable repo (deduplicated)
        let repos_with_subdirs: HashSet<_> = destinations.iter().map(|d| &d.repo_name).collect();
        for repo in config.repos.iter().filter(|r| r.enabled && !r.read_only) {
            if repos_with_subdirs.contains(&repo.name) {
                menu.push(CreateMenuItem::AddSubdir {
                    repo_name: repo.name.clone(),
                });
            }
        }

        // Add clone repo option
        menu.push(CreateMenuItem::CloneRepo);
        menu.push(CreateMenuItem::Cancel);

        match FzfWrapper::builder()
            .prompt(format!("Select destination for {}: ", display))
            .args(fzf_mocha_args())
            .responsive_layout()
            .select(menu)?
        {
            FzfResult::Selected(CreateMenuItem::Destination(item)) => {
                return handle_destination_selected(&config, path, display, &item);
            }
            FzfResult::Selected(CreateMenuItem::AddSubdir { repo_name }) => {
                if handle_add_subdir(&config, &repo_name)? {
                    continue; // Loop back to show updated menu
                }
                return Ok(());
            }
            FzfResult::Selected(CreateMenuItem::CloneRepo) => {
                if handle_clone_repo()? {
                    continue; // Loop back to show updated menu
                }
                return Ok(());
            }
            FzfResult::Selected(CreateMenuItem::Cancel) | FzfResult::Cancelled => {
                emit_cancelled();
                return Ok(());
            }
            FzfResult::Error(e) => return Err(anyhow::anyhow!("Selection error: {}", e)),
            _ => return Ok(()),
        }
    }
}

fn handle_destination_selected(
    config: &Config,
    path: &Path,
    display: &str,
    item: &SourceOption,
) -> Result<()> {
    if item.exists {
        let mut overrides = OverrideConfig::load()?;
        overrides.set_override(
            path.to_path_buf(),
            item.source.repo_name.clone(),
            item.source.subdir_name.clone(),
        )?;
        emit(
            Level::Success,
            "dot.alternative.set",
            &format!(
                "{} {} now sourced from {} / {}",
                char::from(NerdFont::Check),
                display.cyan(),
                item.source.repo_name.green(),
                item.source.subdir_name.green()
            ),
            None,
        );
    } else {
        let db = Database::new(config.database_path().to_path_buf())?;
        add_to_destination(config, &db, path, &item.source)?;

        let mut overrides = OverrideConfig::load()?;
        overrides.set_override(
            path.to_path_buf(),
            item.source.repo_name.clone(),
            item.source.subdir_name.clone(),
        )?;
    }
    Ok(())
}

/// Handle adding a new subdir to a repo. Returns true if successful (should refresh menu).
fn handle_add_subdir(config: &Config, repo_name: &str) -> Result<bool> {
    use crate::dot::localrepo::LocalRepo;

    let new_dir = match FzfWrapper::builder()
        .prompt("New dotfile directory name: ")
        .args(fzf_mocha_args())
        .input()
        .input_result()?
    {
        FzfResult::Selected(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => return Ok(false),
    };

    let local_repo = LocalRepo::new(config, repo_name.to_string())?;
    let local_path = local_repo.local_path(config)?;

    match crate::dot::meta::add_dots_dir(&local_path, &new_dir) {
        Ok(()) => {
            // Also add to global config's active_subdirectories so it shows up in the menu
            let mut config = Config::load(None)?;
            if let Some(repo) = config.repos.iter_mut().find(|r| r.name == repo_name)
                && !repo.active_subdirectories.contains(&new_dir)
            {
                repo.active_subdirectories.push(new_dir.clone());
                config.save(None)?;
            }

            emit(
                Level::Success,
                "dot.alternative.subdir_created",
                &format!(
                    "{} Created dotfile directory '{}/{}' - now select it",
                    char::from(NerdFont::Check),
                    repo_name.green(),
                    new_dir.green()
                ),
                None,
            );
            Ok(true)
        }
        Err(e) => {
            emit(
                Level::Error,
                "dot.alternative.subdir_error",
                &format!(
                    "{} Failed to create directory: {}",
                    char::from(NerdFont::CrossCircle),
                    e
                ),
                None,
            );
            Ok(false)
        }
    }
}

/// Handle cloning a new repo. Returns true if successful (should refresh menu).
fn handle_clone_repo() -> Result<bool> {
    // Delegate to the existing add_repo menu flow
    let config = Config::load(None)?;
    let db = Database::new(config.database_path().to_path_buf())?;

    crate::dot::menu::add_repo::handle_add_repo(&config, &db, false)?;

    // Check if a new repo was added by comparing counts
    let new_config = Config::load(None)?;
    Ok(new_config.repos.len() > config.repos.len())
}

/// Pick a new file to track using the yazi file picker.
fn pick_new_file_to_track() -> Result<Option<std::path::PathBuf>> {
    use crate::menu_utils::{FilePickerScope, MenuWrapper};

    let home = discovery::home_dir();

    match MenuWrapper::file_picker()
        .start_dir(&home)
        .scope(FilePickerScope::Files)
        .hint("Select a file to track as a dotfile")
        .show_hidden(true)
        .pick_one()
    {
        Ok(Some(path)) => {
            // Validate the file is under home directory
            if !path.starts_with(&home) {
                emit(
                    Level::Warn,
                    "dot.alternative.not_in_home",
                    &format!(
                        "{} File must be in your home directory",
                        char::from(NerdFont::Warning)
                    ),
                    None,
                );
                return Ok(None);
            }
            Ok(Some(path))
        }
        Ok(None) => Ok(None),
        Err(e) => {
            emit(
                Level::Error,
                "dot.alternative.picker_error",
                &format!(
                    "{} File picker error: {}",
                    char::from(NerdFont::CrossCircle),
                    e
                ),
                None,
            );
            Ok(None)
        }
    }
}

fn list_file(path: &Path, display: &str, sources: &[DotfileSource]) -> Result<()> {
    if sources.is_empty() {
        emit(
            Level::Info,
            "dot.alternative.list.empty",
            &format!("No alternatives found for {}", display.cyan()),
            None,
        );
        return Ok(());
    }

    let overrides = OverrideConfig::load()?;
    emit(
        Level::Info,
        "dot.alternative.list.header",
        &format!(
            "{} Alternatives for {}:",
            char::from(NerdFont::List),
            display.cyan()
        ),
        None,
    );
    print_sources(path, display, sources, &overrides);
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn show_single_source(config: &Config, display: &str, source: &DotfileSource) -> Result<()> {
    emit(
        Level::Info,
        "dot.alternative.single_source",
        &format!(
            "{} {} is sourced from {} / {}",
            char::from(NerdFont::Check),
            display.cyan(),
            source.repo_name.green(),
            source.subdir_name.green()
        ),
        None,
    );

    let other_dests: Vec<_> = get_destinations(config)
        .into_iter()
        .filter(|d| d.repo_name != source.repo_name || d.subdir_name != source.subdir_name)
        .collect();

    if other_dests.is_empty() {
        emit(
            Level::Info,
            "dot.alternative.no_other_repos",
            &format!(
                "   No other writable repos. Add one with {}",
                "ins dot repo clone <url>".cyan()
            ),
            None,
        );
    } else {
        emit(
            Level::Info,
            "dot.alternative.hint",
            &format!(
                "   {} To create alternative: {}",
                char::from(NerdFont::Info),
                format!("ins dot alternative {} --create", display).dimmed()
            ),
            None,
        );
    }
    Ok(())
}

fn print_sources(
    path: &Path,
    display: &str,
    sources: &[DotfileSource],
    overrides: &OverrideConfig,
) {
    let current = overrides.get_override(path);
    let last = sources.len().saturating_sub(1);

    emit(
        Level::Info,
        "dot.alternative.file",
        &format!("\n  {}", display.cyan()),
        None,
    );

    for (i, source) in sources.iter().enumerate() {
        let is_override = current
            .map(|o| o.source_repo == source.repo_name && o.source_subdir == source.subdir_name)
            .unwrap_or(false);
        let is_default = current.is_none() && i == last;

        let status = if is_override {
            " (current override)".yellow().to_string()
        } else if is_default {
            " (current default)".dimmed().to_string()
        } else {
            String::new()
        };

        emit(
            Level::Info,
            "dot.alternative.source",
            &format!(
                "    - {} / {}{}",
                source.repo_name.green(),
                source.subdir_name.green(),
                status
            ),
            None,
        );
    }
}

fn emit_cancelled() {
    emit(
        Level::Info,
        "dot.alternative.cancelled",
        &format!("{} Selection cancelled", char::from(NerdFont::Info)),
        None,
    );
}
