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
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, Header, MenuCursor};
use crate::ui::catppuccin::fzf_mocha_args;
use crate::ui::prelude::*;

use apply::{is_safe_to_switch, remove_override, reset_override, set_alternative};
use discovery::{DiscoveryFilter, discover_dotfiles, get_destinations, to_display_path};
use picker::{BrowseMenuItem, CreateMenuItem, MenuItem, SourceOption};

// Re-export for external use (add command uses these)
pub use apply::add_to_destination;

// ─────────────────────────────────────────────────────────────────────────────
// Flow Control
// ─────────────────────────────────────────────────────────────────────────────

/// Explicit control flow for menu operations.
/// This replaces confusing `Result<bool>` patterns.
enum Flow {
    /// Continue showing the current menu (refresh and loop)
    Continue,
    /// Action completed successfully, exit current menu
    Done,
    /// User cancelled, exit current menu
    Cancelled,
}

/// Show a message and return the appropriate flow
fn message_and_continue(msg: &str) -> Result<Flow> {
    FzfWrapper::message(msg)?;
    Ok(Flow::Continue)
}

fn message_and_done(msg: &str) -> Result<Flow> {
    FzfWrapper::message(msg)?;
    Ok(Flow::Done)
}

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────

/// Pick a destination and add a file there (shared by `add --choose` and `alternative --create`).
pub fn pick_destination_and_add(config: &Config, path: &Path) -> Result<bool> {
    let display = to_display_path(path);
    let existing = find_all_sources(config, path)?;
    match run_create_flow(path, &display, &existing)? {
        Flow::Done => Ok(true),
        _ => Ok(false),
    }
}

/// CLI action for the alternative command.
pub enum Action {
    /// Interactive source selection menu
    Select,
    /// Interactive destination picker for creating alternatives
    Create,
    /// Non-interactive: list alternatives
    List,
    /// Non-interactive: reset/remove override
    Reset,
    /// Non-interactive: set source to specific repo[/subdir]
    SetDirect {
        repo: String,
        subdir: Option<String>,
    },
    /// Non-interactive: create at specific repo/subdir
    CreateDirect { repo: String, subdir: String },
}

impl Action {
    pub fn from_flags(
        reset: bool,
        create: bool,
        list: bool,
        set: Option<&str>,
        repo: Option<&str>,
        subdir: Option<&str>,
    ) -> Self {
        if reset {
            Self::Reset
        } else if let Some(set_value) = set {
            // Parse "repo" or "repo/subdir" format
            let (repo, subdir) = if let Some(idx) = set_value.find('/') {
                let (r, s) = set_value.split_at(idx);
                (r.to_string(), Some(s[1..].to_string()))
            } else {
                (set_value.to_string(), None)
            };
            Self::SetDirect { repo, subdir }
        } else if create {
            if let Some(repo_name) = repo {
                // Non-interactive create with explicit destination
                let subdir_name = subdir.unwrap_or("dots").to_string();
                Self::CreateDirect {
                    repo: repo_name.to_string(),
                    subdir: subdir_name,
                }
            } else {
                // Interactive create
                Self::Create
            }
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
    set: Option<&str>,
    repo: Option<&str>,
    subdir: Option<&str>,
) -> Result<()> {
    let action = Action::from_flags(reset, create, list, set, repo, subdir);
    let target_path = resolve_dotfile_path(path)?;
    let display_path = to_display_path(&target_path);

    if target_path.is_dir() {
        return handle_directory(config, &target_path, &display_path, action);
    }

    handle_file(config, &target_path, &display_path, action)
}

// ─────────────────────────────────────────────────────────────────────────────
// Directory Handling
// ─────────────────────────────────────────────────────────────────────────────

fn handle_directory(config: &Config, dir: &Path, display: &str, action: Action) -> Result<()> {
    match action {
        Action::Reset => Err(anyhow::anyhow!(
            "--reset is not supported for directories. Use it with a specific file."
        )),
        Action::SetDirect { .. } => Err(anyhow::anyhow!(
            "--set is not supported for directories. Use it with a specific file."
        )),
        Action::CreateDirect { .. } => Err(anyhow::anyhow!(
            "--create with --repo is not supported for directories. Use it with a specific file."
        )),
        Action::List => list_directory(config, dir, display),
        Action::Select => run_browse_menu(dir, display, BrowseMode::SelectAlternative),
        Action::Create => run_browse_menu(dir, display, BrowseMode::CreateAlternative),
    }
}

#[derive(Clone, Copy)]
enum BrowseMode {
    SelectAlternative,
    CreateAlternative,
}

fn run_browse_menu(dir: &Path, display: &str, mode: BrowseMode) -> Result<()> {
    let filter = match mode {
        BrowseMode::SelectAlternative => DiscoveryFilter::WithAlternatives,
        BrowseMode::CreateAlternative => DiscoveryFilter::All,
    };

    // Check once at the start
    let config = Config::load(None)?;
    let initial_dotfiles = discover_dotfiles(&config, dir, filter)?;

    if initial_dotfiles.is_empty() {
        return match mode {
            BrowseMode::CreateAlternative => {
                emit(
                    Level::Info,
                    "dot.alternative.empty",
                    &format!("No dotfiles found in {}", display.cyan()),
                    None,
                );
                Ok(())
            }
            BrowseMode::SelectAlternative => offer_create_alternative(dir, display),
        };
    }

    // Main menu loop
    let mut cursor = MenuCursor::new();
    let mut preselect: Option<String> = None;

    loop {
        let config = Config::load(None)?;
        let dotfiles = discover_dotfiles(&config, dir, filter)?;

        let action_text = match mode {
            BrowseMode::SelectAlternative => "switch source",
            BrowseMode::CreateAlternative => "create alternative",
        };

        emit(
            Level::Info,
            "dot.alternative.found",
            &format!(
                "{} Found {} dotfiles in {} (select to {})",
                char::from(NerdFont::Check),
                dotfiles.len(),
                display.cyan(),
                action_text
            ),
            None,
        );

        // Build menu
        let mut menu: Vec<BrowseMenuItem> = Vec::new();
        if matches!(mode, BrowseMode::CreateAlternative) {
            menu.push(BrowseMenuItem::PickNewFile);
        }
        menu.push(BrowseMenuItem::Cancel);
        menu.extend(dotfiles.into_iter().map(BrowseMenuItem::Dotfile));

        let mut builder = FzfWrapper::builder()
            .prompt(format!("Select dotfile in {}: ", display))
            .args(fzf_mocha_args())
            .responsive_layout();

        if let Some(index) = cursor.initial_index(&menu) {
            builder = builder.initial_index(index);
        }

        if let Some(q) = preselect.take() {
            builder = builder.query(q);
        }

        match builder.select(menu.clone())? {
            FzfResult::Selected(BrowseMenuItem::Dotfile(selected)) => {
                cursor.update(&BrowseMenuItem::Dotfile(selected.clone()), &menu);
                let result = match mode {
                    BrowseMode::CreateAlternative => {
                        let sources = find_all_sources(&config, &selected.target_path)?;
                        run_create_flow(&selected.target_path, &selected.display_path, &sources)?
                    }
                    BrowseMode::SelectAlternative => {
                        run_select_flow(&selected.target_path, &selected.display_path)?
                    }
                };

                match result {
                    Flow::Done => {
                        // In create mode, stay in menu to allow more operations
                        if matches!(mode, BrowseMode::CreateAlternative) {
                            preselect = Some(selected.display_path);
                            continue;
                        }
                        return Ok(());
                    }
                    Flow::Continue => continue,
                    Flow::Cancelled => return Ok(()),
                }
            }
            FzfResult::Selected(BrowseMenuItem::PickNewFile) => {
                cursor.update(&BrowseMenuItem::PickNewFile, &menu);
                if let Some(path) = pick_new_file_to_track()? {
                    let file_display = to_display_path(&path);
                    let sources = find_all_sources(&config, &path)?;
                    let create_result = run_create_flow(&path, &file_display, &sources)?;
                    if matches!(create_result, Flow::Done) {
                        preselect = Some(file_display);
                    } else if matches!(create_result, Flow::Cancelled) {
                        preselect = Some(file_display);
                    }
                }
                continue;
            }
            FzfResult::Selected(BrowseMenuItem::Cancel) => {
                cursor.update(&BrowseMenuItem::Cancel, &menu);
                emit_cancelled();
                return Ok(());
            }
            FzfResult::Cancelled => {
                emit_cancelled();
                return Ok(());
            }
            FzfResult::Error(e) => return Err(anyhow::anyhow!("Selection error: {}", e)),
            _ => return Ok(()),
        }
    }
}

fn offer_create_alternative(dir: &Path, display: &str) -> Result<()> {
    #[derive(Clone)]
    enum Choice {
        Create,
        Cancel,
    }

    impl FzfSelectable for Choice {
        fn fzf_display_text(&self) -> String {
            use crate::ui::catppuccin::{colors, format_icon_colored};
            match self {
                Choice::Create => format!(
                    "{} Create new alternative...",
                    format_icon_colored(NerdFont::Plus, colors::GREEN)
                ),
                Choice::Cancel => format!(
                    "{} Cancel",
                    format_icon_colored(NerdFont::Cross, colors::OVERLAY0)
                ),
            }
        }
        fn fzf_key(&self) -> String {
            match self {
                Choice::Create => "create".into(),
                Choice::Cancel => "cancel".into(),
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

    match FzfWrapper::builder()
        .header(Header::fancy("No alternatives found"))
        .prompt("Select action: ")
        .args(fzf_mocha_args())
        .responsive_layout()
        .select(vec![Choice::Create, Choice::Cancel])?
    {
        FzfResult::Selected(Choice::Create) => {
            run_browse_menu(dir, display, BrowseMode::CreateAlternative)
        }
        _ => {
            emit_cancelled();
            Ok(())
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// File Handling
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
            run_create_flow(path, display, &sources)?;
            Ok(())
        }
        Action::Select => {
            run_select_flow(path, display)?;
            Ok(())
        }
        Action::SetDirect { repo, subdir } => {
            handle_set_direct(config, path, display, &repo, subdir.as_deref())
        }
        Action::CreateDirect { repo, subdir } => {
            handle_create_direct(config, path, display, &repo, &subdir)
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Non-Interactive Handlers (--set and --create --repo)
// ─────────────────────────────────────────────────────────────────────────────

/// Handle --set REPO[/SUBDIR] flag (non-interactive).
fn handle_set_direct(
    config: &Config,
    path: &Path,
    display: &str,
    repo_name: &str,
    subdir: Option<&str>,
) -> Result<()> {
    // Find all sources for this file
    let sources = find_all_sources(config, path)?;

    if sources.is_empty() {
        return Err(anyhow::anyhow!(
            "No sources found for {}. Use --create to add it first.",
            display
        ));
    }

    // Find matching source(s) in the specified repo
    let matching_sources: Vec<_> = sources
        .iter()
        .filter(|s| s.repo_name == repo_name)
        .collect();

    if matching_sources.is_empty() {
        let available: Vec<_> = sources.iter().map(|s| &s.repo_name).collect();
        return Err(anyhow::anyhow!(
            "Repository '{}' does not contain {}.\nAvailable sources: {:?}",
            repo_name,
            display,
            available
        ));
    }

    // Resolve which source to use
    let source = if let Some(subdir_name) = subdir {
        // Explicit subdir specified - find exact match
        matching_sources
            .iter()
            .find(|s| s.subdir_name == subdir_name)
            .ok_or_else(|| {
                let available_subdirs: Vec<_> =
                    matching_sources.iter().map(|s| &s.subdir_name).collect();
                anyhow::anyhow!(
                    "Subdir '{}' in '{}' does not contain {}.\nAvailable subdirs: {:?}",
                    subdir_name,
                    repo_name,
                    display,
                    available_subdirs
                )
            })?
    } else {
        // No subdir specified - use first match
        matching_sources.first().ok_or_else(|| {
            anyhow::anyhow!("Repository '{}' does not contain {}", repo_name, display)
        })?
    };

    // Create SourceOption for set_alternative
    let source_option = picker::SourceOption {
        source: (*source).clone(),
        is_current: false,
        exists: true,
    };

    // Check if safe to switch
    let all_options: Vec<_> = sources
        .into_iter()
        .map(|s| picker::SourceOption {
            source: s,
            is_current: false,
            exists: true,
        })
        .collect();

    if !is_safe_to_switch(path, &all_options)? {
        return Err(anyhow::anyhow!(
            "Cannot switch {} - file has been modified. Use 'ins dot reset {}' first.",
            display,
            display
        ));
    }

    // Set the alternative
    set_alternative(config, path, display, &source_option)?;
    Ok(())
}

/// Handle --create --repo REPO --subdir SUBDIR (non-interactive).
fn handle_create_direct(
    config: &Config,
    path: &Path,
    display: &str,
    repo_name: &str,
    subdir_name: &str,
) -> Result<()> {
    // Validate the file exists
    if !path.exists() {
        return Err(anyhow::anyhow!(
            "File does not exist: {}\nCannot create an alternative for a non-existent file.",
            display
        ));
    }

    // Find the destination
    let destinations = get_destinations(config);
    let dest = destinations
        .iter()
        .find(|d| d.repo_name == repo_name && d.subdir_name == subdir_name)
        .ok_or_else(|| {
            let available: Vec<String> = destinations
                .iter()
                .map(|d| format!("{}/{}", d.repo_name, d.subdir_name))
                .collect();
            if available.is_empty() {
                anyhow::anyhow!(
                    "No writable destinations available.\n\
                     Add a writable repository with 'ins dot repo clone <url>'"
                )
            } else {
                anyhow::anyhow!(
                    "Destination '{}/{}' not found.\nAvailable destinations: {}",
                    repo_name,
                    subdir_name,
                    available.join(", ")
                )
            }
        })?;

    // Check if file already exists at destination
    let existing = find_all_sources(config, path)?;
    if existing
        .iter()
        .any(|s| s.repo_name == repo_name && s.subdir_name == subdir_name)
    {
        return Err(anyhow::anyhow!(
            "'{}' already exists at {}/{}.\n\
             Use '--set {}/{}' to switch to it, or choose a different destination.",
            display,
            repo_name,
            subdir_name,
            repo_name,
            subdir_name
        ));
    }

    // Copy file to destination
    let db = Database::new(config.database_path().to_path_buf())?;
    add_to_destination(config, &db, path, dest)?;

    // Set override if multiple sources now exist
    let sources = find_all_sources(config, path)?;
    if sources.len() > 1 {
        let mut overrides = OverrideConfig::load()?;
        overrides.set_override(
            path.to_path_buf(),
            repo_name.to_string(),
            subdir_name.to_string(),
        )?;

        emit(
            Level::Info,
            "dot.alternative.created_with_override",
            &format!(
                "   {} Set as active source ({} alternatives available)",
                char::from(NerdFont::Info),
                sources.len()
            ),
            None,
        );
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Create Flow - Adding a file to a new destination
// ─────────────────────────────────────────────────────────────────────────────

fn run_create_flow(path: &Path, display: &str, existing: &[DotfileSource]) -> Result<Flow> {
    use std::collections::HashSet;

    let mut cursor = MenuCursor::new();

    loop {
        let config = Config::load(None)?;
        let destinations = get_destinations(&config);

        // Build menu
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

        // Add "new subdir" options
        let repos_with_subdirs: HashSet<_> = destinations.iter().map(|d| &d.repo_name).collect();
        for repo in config.repos.iter().filter(|r| r.enabled && !r.read_only) {
            if repos_with_subdirs.contains(&repo.name) {
                menu.push(CreateMenuItem::AddSubdir {
                    repo_name: repo.name.clone(),
                });
            }
        }

        menu.push(CreateMenuItem::CloneRepo);
        menu.push(CreateMenuItem::Cancel);

        let mut builder = FzfWrapper::builder()
            .prompt(format!("Select destination for {}: ", display))
            .args(fzf_mocha_args())
            .responsive_layout();

        if let Some(index) = cursor.initial_index(&menu) {
            builder = builder.initial_index(index);
        }

        match builder.select(menu.clone())? {
            FzfResult::Selected(CreateMenuItem::Destination(item)) => {
                cursor.update(&CreateMenuItem::Destination(item.clone()), &menu);
                match add_file_to_destination(&config, path, display, &item)? {
                    Flow::Continue => continue,
                    other => return Ok(other),
                }
            }
            FzfResult::Selected(CreateMenuItem::AddSubdir { repo_name }) => {
                cursor.update(
                    &CreateMenuItem::AddSubdir {
                        repo_name: repo_name.clone(),
                    },
                    &menu,
                );
                if create_new_subdir(&config, &repo_name)? {
                    continue;
                }
                return Ok(Flow::Cancelled);
            }
            FzfResult::Selected(CreateMenuItem::CloneRepo) => {
                cursor.update(&CreateMenuItem::CloneRepo, &menu);
                if clone_new_repo()? {
                    continue;
                }
                return Ok(Flow::Cancelled);
            }
            FzfResult::Selected(CreateMenuItem::Cancel) => {
                cursor.update(&CreateMenuItem::Cancel, &menu);
                emit_cancelled();
                return Ok(Flow::Cancelled);
            }
            FzfResult::Cancelled => {
                emit_cancelled();
                return Ok(Flow::Cancelled);
            }
            FzfResult::Error(e) => return Err(anyhow::anyhow!("Selection error: {}", e)),
            _ => return Ok(Flow::Cancelled),
        }
    }
}

fn add_file_to_destination(
    config: &Config,
    path: &Path,
    display: &str,
    item: &SourceOption,
) -> Result<Flow> {
    // Already exists at this destination
    if item.exists {
        return message_and_continue(&format!(
            "'{}' already exists at {} / {}\n\n\
            This location is already tracked as an alternative.\n\
            Use the alternative selection menu to switch sources.",
            display, item.source.repo_name, item.source.subdir_name
        ));
    }

    // Open database
    let db = match Database::new(config.database_path().to_path_buf()) {
        Ok(db) => db,
        Err(e) => return message_and_continue(&format!("Failed to open database: {}", e)),
    };

    // Copy the file
    if let Err(e) = add_to_destination(config, &db, path, &item.source) {
        return message_and_continue(&format!(
            "Failed to add '{}' to {} / {}:\n\n{}",
            display, item.source.repo_name, item.source.subdir_name, e
        ));
    }

    // Check how many sources exist now
    let config = Config::load(None)?;
    let sources = find_all_sources(&config, path)?;

    if sources.len() <= 1 {
        // Only one source - just tracking, no override needed
        return message_and_done(&format!(
            "Added '{}' to {} / {}\n\n\
            Note: This file is now tracked, but has no alternatives.\n\
            An override is only needed when multiple sources exist.",
            display, item.source.repo_name, item.source.subdir_name
        ));
    }

    // Multiple sources - set override
    let mut overrides = match OverrideConfig::load() {
        Ok(o) => o,
        Err(e) => {
            return message_and_done(&format!(
                "File was copied but failed to load overrides: {}\n\n\
                Use 'ins dot alternative {}' to switch sources.",
                e, display
            ));
        }
    };

    if let Err(e) = overrides.set_override(
        path.to_path_buf(),
        item.source.repo_name.clone(),
        item.source.subdir_name.clone(),
    ) {
        return message_and_done(&format!(
            "File was copied but failed to set override: {}\n\n\
            Use 'ins dot alternative {}' to switch sources.",
            e, display
        ));
    }

    message_and_done(&format!(
        "Created alternative for '{}' at {} / {}\n\n\
        This location is now set as the active source.\n\
        {} source(s) available.",
        display,
        item.source.repo_name,
        item.source.subdir_name,
        sources.len()
    ))
}

fn create_new_subdir(config: &Config, repo_name: &str) -> Result<bool> {
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
            // Add to global config
            let mut config = Config::load(None)?;
            if let Some(repo) = config.repos.iter_mut().find(|r| r.name == repo_name) {
                let active_subdirs = repo.active_subdirectories.get_or_insert_with(Vec::new);
                if !active_subdirs.contains(&new_dir) {
                    active_subdirs.push(new_dir.clone());
                    config.save(None)?;
                }
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
            FzfWrapper::message(&format!("Failed to create directory: {}", e))?;
            Ok(false)
        }
    }
}

fn clone_new_repo() -> Result<bool> {
    let config = Config::load(None)?;
    let db = Database::new(config.database_path().to_path_buf())?;

    crate::dot::menu::add_repo::handle_add_repo(&config, &db, false)?;

    let new_config = Config::load(None)?;
    Ok(new_config.repos.len() > config.repos.len())
}

// ─────────────────────────────────────────────────────────────────────────────
// Select Flow - Switching between existing alternatives
// ─────────────────────────────────────────────────────────────────────────────

fn run_select_flow(path: &Path, display: &str) -> Result<Flow> {
    let config = Config::load(None)?;
    let sources = find_all_sources(&config, path)?;

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
        return Ok(Flow::Cancelled);
    }

    // Check for unnecessary override (1 source but has override)
    let overrides = OverrideConfig::load()?;
    let has_override = overrides.get_override(path).is_some();

    if sources.len() == 1 {
        return handle_single_source(path, display, &sources[0], has_override);
    }

    // Multiple sources - show selection menu
    run_source_selection_menu(path, display, sources, &overrides)
}

fn handle_single_source(
    path: &Path,
    display: &str,
    source: &DotfileSource,
    has_override: bool,
) -> Result<Flow> {
    if has_override {
        // Unnecessary override - offer to remove it
        #[derive(Clone)]
        enum Choice {
            Remove,
            Back,
        }

        impl FzfSelectable for Choice {
            fn fzf_display_text(&self) -> String {
                use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored};
                match self {
                    Choice::Remove => format!(
                        "{} Remove unnecessary override",
                        format_icon_colored(NerdFont::Trash, colors::YELLOW)
                    ),
                    Choice::Back => format!("{} Back", format_back_icon()),
                }
            }
            fn fzf_key(&self) -> String {
                match self {
                    Choice::Remove => "remove".into(),
                    Choice::Back => "back".into(),
                }
            }
        }

        match FzfWrapper::builder()
            .header(Header::fancy(&format!(
                "{} (1 source, has unnecessary override)",
                display
            )))
            .prompt("Action: ")
            .args(fzf_mocha_args())
            .responsive_layout()
            .select(vec![Choice::Remove, Choice::Back])?
        {
            FzfResult::Selected(Choice::Remove) => {
                let mut overrides = OverrideConfig::load()?;
                overrides.remove_override(path)?;
                return message_and_done(&format!(
                    "Removed override for '{}'\n\nThe file is still tracked at {} / {}",
                    display, source.repo_name, source.subdir_name
                ));
            }
            _ => return Ok(Flow::Cancelled),
        }
    }

    // Normal single source - show info
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

    let config = Config::load(None)?;
    let other_dests: Vec<_> = get_destinations(&config)
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
    Ok(Flow::Done)
}

fn run_source_selection_menu(
    path: &Path,
    display: &str,
    sources: Vec<DotfileSource>,
    overrides: &OverrideConfig,
) -> Result<Flow> {
    let current = overrides.get_override(path);
    let default_source = sources.last().cloned();
    let mut cursor = MenuCursor::new();

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
        return Ok(Flow::Cancelled);
    }

    loop {
        let mut menu: Vec<MenuItem> = items.clone().into_iter().map(MenuItem::Source).collect();

        // Add Create Alternative option
        menu.push(MenuItem::CreateAlternative);

        if current.is_some()
            && let Some(default) = default_source.clone()
        {
            menu.push(MenuItem::RemoveOverride {
                default_source: default,
            });
        }
        menu.push(MenuItem::Back);

        let config = Config::load(None)?;
        let mut builder = FzfWrapper::builder()
            .prompt(format!("Select source for {}: ", display))
            .args(fzf_mocha_args())
            .responsive_layout();

        if let Some(index) = cursor.initial_index(&menu) {
            builder = builder.initial_index(index);
        }

        match builder.select(menu.clone())? {
            FzfResult::Selected(MenuItem::Source(item)) => {
                cursor.update(&MenuItem::Source(item.clone()), &menu);
                set_alternative(&config, path, display, &item)?;
                return Ok(Flow::Done);
            }
            FzfResult::Selected(MenuItem::CreateAlternative) => {
                cursor.update(&MenuItem::CreateAlternative, &menu);
                let sources = find_all_sources(&config, path)?;
                match run_create_flow(path, display, &sources)? {
                    Flow::Continue => continue,
                    other => return Ok(other),
                }
            }
            FzfResult::Selected(MenuItem::RemoveOverride { default_source }) => {
                cursor.update(
                    &MenuItem::RemoveOverride {
                        default_source: default_source.clone(),
                    },
                    &menu,
                );
                remove_override(&config, path, display, &default_source)?;
                return Ok(Flow::Done);
            }
            FzfResult::Selected(MenuItem::Back) => {
                cursor.update(&MenuItem::Back, &menu);
                return Ok(Flow::Cancelled);
            }
            FzfResult::Cancelled => return Ok(Flow::Cancelled),
            FzfResult::Error(e) => return Err(anyhow::anyhow!("Selection error: {}", e)),
            _ => return Ok(Flow::Cancelled),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Utility Functions
// ─────────────────────────────────────────────────────────────────────────────

fn pick_new_file_to_track() -> Result<Option<std::path::PathBuf>> {
    use crate::menu_utils::{FilePickerScope, MenuWrapper};

    let home = discovery::home_dir();

    match MenuWrapper::file_picker()
        .start_dir(&home)
        .scope(FilePickerScope::Files)
        .show_hidden(true)
        .hint("Select a file to track as a dotfile")
        .pick_one()
    {
        Ok(Some(path)) => {
            if !path.starts_with(&home) {
                FzfWrapper::message("File must be in your home directory")?;
                return Ok(None);
            }
            Ok(Some(path))
        }
        Ok(None) => Ok(None),
        Err(e) => {
            FzfWrapper::message(&format!("File picker error: {}", e))?;
            Ok(None)
        }
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
