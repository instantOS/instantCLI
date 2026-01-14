//! Alternative source selection for dotfiles
//!
//! Allows users to interactively select which repository/subdirectory
//! a dotfile should be sourced from.

use anyhow::Result;
use colored::Colorize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::dot::config::Config;
use crate::dot::localrepo::LocalRepo;
use crate::dot::override_config::{DotfileSource, OverrideConfig, find_all_sources};
use crate::dot::utils::resolve_dotfile_path;
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper};
use crate::ui::catppuccin::{colors, format_icon_colored, fzf_mocha_args};
use crate::ui::prelude::*;
use crate::ui::preview::PreviewBuilder;

/// Menu item for alternative selection
#[derive(Clone)]
enum AlternativeMenuItem {
    /// Select a specific source
    Source(SourceSelectItem),
    /// Remove the current override (revert to default)
    RemoveOverride {
        /// The source that will become active after removing override
        default_source: DotfileSource,
    },
}

/// Wrapper for DotfileSource to implement FzfSelectable
#[derive(Clone)]
struct SourceSelectItem {
    source: DotfileSource,
    is_current: bool,
    exists: bool,
}

impl FzfSelectable for AlternativeMenuItem {
    fn fzf_display_text(&self) -> String {
        match self {
            AlternativeMenuItem::Source(item) => {
                let current = if item.is_current { " (current)" } else { "" };
                let status = if item.exists { "" } else { " [new]" };
                format!(
                    "{} {} / {}{}{}",
                    format_icon_colored(NerdFont::Folder, colors::MAUVE),
                    item.source.repo_name,
                    item.source.subdir_name,
                    current,
                    status
                )
            }
            AlternativeMenuItem::RemoveOverride { default_source } => {
                format!(
                    "{} Remove Override → {} / {}",
                    format_icon_colored(NerdFont::Trash, colors::RED),
                    default_source.repo_name,
                    default_source.subdir_name
                )
            }
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            AlternativeMenuItem::Source(item) => {
                format!("{}:{}", item.source.repo_name, item.source.subdir_name)
            }
            AlternativeMenuItem::RemoveOverride { .. } => "!__remove_override__".to_string(),
        }
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        match self {
            AlternativeMenuItem::Source(item) => {
                let mut builder = PreviewBuilder::new().header(
                    NerdFont::Folder,
                    &format!("{} / {}", item.source.repo_name, item.source.subdir_name),
                );

                if item.is_current {
                    builder = builder.blank().line(
                        colors::GREEN,
                        Some(NerdFont::Check),
                        "Currently selected source",
                    );
                }

                if !item.exists {
                    builder = builder.blank().line(
                        colors::YELLOW,
                        Some(NerdFont::Plus),
                        "File will be created in this location",
                    );
                }

                builder = builder.blank().line(
                    colors::TEXT,
                    Some(NerdFont::File),
                    &format!("Path: {}", item.source.source_path.display()),
                );

                crate::menu::protocol::FzfPreview::Text(builder.build_string())
            }
            AlternativeMenuItem::RemoveOverride { default_source } => {
                crate::menu::protocol::FzfPreview::Text(
                    PreviewBuilder::new()
                        .header(NerdFont::Trash, "Remove Override")
                        .blank()
                        .text("Remove the manual override for this file.")
                        .blank()
                        .line(
                            colors::PEACH,
                            Some(NerdFont::ArrowRight),
                            "After removal, the file will be sourced from:",
                        )
                        .indented_line(
                            colors::GREEN,
                            None,
                            &format!(
                                "{} / {}",
                                default_source.repo_name, default_source.subdir_name
                            ),
                        )
                        .blank()
                        .text("This is the default based on repository priority.")
                        .build_string(),
                )
            }
        }
    }
}

impl FzfSelectable for SourceSelectItem {
    fn fzf_display_text(&self) -> String {
        let current = if self.is_current { " (current)" } else { "" };
        let status = if self.exists { "" } else { " [new]" };
        format!(
            "{} / {}{}{}",
            self.source.repo_name, self.source.subdir_name, current, status
        )
    }

    fn fzf_key(&self) -> String {
        format!("{}:{}", self.source.repo_name, self.source.subdir_name)
    }
}

/// A dotfile that has multiple sources available
#[derive(Clone)]
struct DotfileWithAlternatives {
    /// Target path in home directory
    target_path: PathBuf,
    /// Display path (with ~ prefix)
    display_path: String,
    /// All available sources
    sources: Vec<DotfileSource>,
}

impl FzfSelectable for DotfileWithAlternatives {
    fn fzf_display_text(&self) -> String {
        format!(
            "{} {} ({} alternatives)",
            format_icon_colored(NerdFont::File, colors::SKY),
            self.display_path,
            self.sources.len()
        )
    }

    fn fzf_key(&self) -> String {
        self.display_path.clone()
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        let mut builder = PreviewBuilder::new()
            .header(NerdFont::File, &self.display_path)
            .blank()
            .line(colors::MAUVE, Some(NerdFont::List), "Available sources:");

        for (i, source) in self.sources.iter().enumerate() {
            builder = builder.indented_line(
                colors::TEXT,
                None,
                &format!("{}. {} / {}", i + 1, source.repo_name, source.subdir_name),
            );
        }

        crate::menu::protocol::FzfPreview::Text(builder.build_string())
    }
}

/// Find all dotfiles within a directory that have multiple sources (alternatives)
///
/// This is done efficiently by scanning the dotfile repos first, NOT the target directory.
fn find_dotfiles_with_alternatives_in_dir(
    config: &Config,
    dir_path: &Path,
) -> Result<Vec<DotfileWithAlternatives>> {
    let home = PathBuf::from(shellexpand::tilde("~").to_string());

    // Map from target_path to list of sources
    let mut sources_by_target: HashMap<PathBuf, Vec<DotfileSource>> = HashMap::new();

    // Scan all repos and subdirs for dotfiles
    for repo_config in &config.repos {
        if !repo_config.enabled {
            continue;
        }

        let local_repo = match LocalRepo::new(config, repo_config.name.clone()) {
            Ok(repo) => repo,
            Err(_) => continue,
        };

        for dotfile_dir in &local_repo.dotfile_dirs {
            // Walk the dotfile directory
            for entry in WalkDir::new(&dotfile_dir.path)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let path_str = e.path().to_string_lossy();
                    !path_str.contains("/.git/") && e.file_type().is_file()
                })
            {
                let source_path = entry.path().to_path_buf();

                // Calculate the target path
                let relative_path = match source_path.strip_prefix(&dotfile_dir.path) {
                    Ok(rel) => rel,
                    Err(_) => continue,
                };
                let target_path = home.join(relative_path);

                // Only include files that would be in the specified directory
                if !target_path.starts_with(dir_path) {
                    continue;
                }

                let subdir_name = dotfile_dir
                    .path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();

                sources_by_target
                    .entry(target_path)
                    .or_default()
                    .push(DotfileSource {
                        repo_name: repo_config.name.clone(),
                        subdir_name,
                        source_path,
                    });
            }
        }
    }

    // Filter to only dotfiles with multiple sources
    let mut results: Vec<DotfileWithAlternatives> = sources_by_target
        .into_iter()
        .filter(|(_, sources)| sources.len() > 1)
        .map(|(target_path, sources)| {
            let display_path = target_path
                .strip_prefix(&home)
                .map(|p| format!("~/{}", p.display()))
                .unwrap_or_else(|_| target_path.display().to_string());
            DotfileWithAlternatives {
                target_path,
                display_path,
                sources,
            }
        })
        .collect();

    // Sort by display path for consistent ordering
    results.sort_by(|a, b| a.display_path.cmp(&b.display_path));

    Ok(results)
}

/// Handle alternative selection for a directory (browse mode)
fn handle_directory_alternatives(config: &Config, dir_path: &Path) -> Result<()> {
    let home = PathBuf::from(shellexpand::tilde("~").to_string());
    let display_dir = dir_path
        .strip_prefix(&home)
        .map(|p| format!("~/{}", p.display()))
        .unwrap_or_else(|_| dir_path.display().to_string());

    // Find all dotfiles with alternatives in this directory
    let dotfiles = find_dotfiles_with_alternatives_in_dir(config, dir_path)?;

    if dotfiles.is_empty() {
        emit(
            Level::Info,
            "dot.alternative.dir.no_alternatives",
            &format!(
                "{} No dotfiles with alternatives found in {}",
                char::from(NerdFont::Info),
                display_dir.cyan()
            ),
            None,
        );
        return Ok(());
    }

    emit(
        Level::Info,
        "dot.alternative.dir.found",
        &format!(
            "{} Found {} dotfiles with alternatives in {}",
            char::from(NerdFont::Check),
            dotfiles.len(),
            display_dir.cyan()
        ),
        None,
    );

    // Show picker to select a dotfile
    let prompt = format!("Select dotfile in {}: ", display_dir);
    let selected = match FzfWrapper::builder()
        .prompt(prompt)
        .args(fzf_mocha_args())
        .responsive_layout()
        .select(dotfiles)?
    {
        FzfResult::Selected(item) => item,
        FzfResult::Cancelled => {
            emit(
                Level::Info,
                "dot.alternative.cancelled",
                &format!("{} Selection cancelled", char::from(NerdFont::Info)),
                None,
            );
            return Ok(());
        }
        FzfResult::Error(e) => {
            return Err(anyhow::anyhow!("Selection error: {}", e));
        }
        _ => return Ok(()),
    };

    // Now handle the selected file using the existing logic
    handle_file_alternative(config, &selected.target_path, &selected.display_path)
}

/// Handle --list flag for a directory (list all alternatives for all dotfiles)
fn handle_directory_list(config: &Config, dir_path: &Path) -> Result<()> {
    let home = PathBuf::from(shellexpand::tilde("~").to_string());
    let display_dir = dir_path
        .strip_prefix(&home)
        .map(|p| format!("~/{}", p.display()))
        .unwrap_or_else(|_| dir_path.display().to_string());

    // Find all dotfiles with alternatives in this directory
    let dotfiles = find_dotfiles_with_alternatives_in_dir(config, dir_path)?;

    if dotfiles.is_empty() {
        emit(
            Level::Info,
            "dot.alternative.dir.list.empty",
            &format!(
                "{} No dotfiles with alternatives found in {}",
                char::from(NerdFont::Info),
                display_dir.cyan()
            ),
            None,
        );
        return Ok(());
    }

    let overrides = OverrideConfig::load()?;

    emit(
        Level::Info,
        "dot.alternative.dir.list.header",
        &format!(
            "{} Alternatives for {} dotfiles in {}:",
            char::from(NerdFont::List),
            dotfiles.len(),
            display_dir.cyan()
        ),
        None,
    );

    for dotfile in &dotfiles {
        let current_override = overrides.get_override(&dotfile.target_path);
        let last_source_index = dotfile.sources.len() - 1;

        emit(
            Level::Info,
            "dot.alternative.dir.list.file",
            &format!("\n  {}", dotfile.display_path.cyan()),
            None,
        );

        for (i, source) in dotfile.sources.iter().enumerate() {
            let is_override = current_override
                .map(|o| o.source_repo == source.repo_name && o.source_subdir == source.subdir_name)
                .unwrap_or(false);

            let is_default = current_override.is_none() && i == last_source_index;

            let status = if is_override {
                " (current override)".yellow().to_string()
            } else if is_default {
                " (current default)".dimmed().to_string()
            } else {
                "".to_string()
            };

            emit(
                Level::Info,
                "dot.alternative.dir.list.item",
                &format!(
                    "    • {} / {}{}",
                    source.repo_name.green(),
                    source.subdir_name.green(),
                    status
                ),
                None,
            );
        }
    }
    Ok(())
}

/// Handle alternative selection for a specific file
fn handle_file_alternative(config: &Config, target_path: &Path, display_path: &str) -> Result<()> {
    // Find all sources for this file
    let sources = find_all_sources(config, target_path)?;

    if sources.is_empty() {
        emit(
            Level::Warn,
            "dot.alternative.not_found",
            &format!(
                "{} No dotfile sources found for {}. Use --create to add it to a repo.",
                char::from(NerdFont::Warning),
                display_path.yellow()
            ),
            None,
        );
        return Ok(());
    }

    if sources.len() == 1 {
        let source = &sources[0];

        // Find other destinations where this file could be added
        let all_destinations = get_all_destinations(config)?;
        let other_destinations: Vec<_> = all_destinations
            .into_iter()
            .filter(|d| d.repo_name != source.repo_name || d.subdir_name != source.subdir_name)
            .collect();

        emit(
            Level::Info,
            "dot.alternative.single_source",
            &format!(
                "{} {} is sourced from {} / {}",
                char::from(NerdFont::Check),
                display_path.cyan(),
                source.repo_name.green(),
                source.subdir_name.green()
            ),
            None,
        );

        if other_destinations.is_empty() {
            // No other writable repos available
            emit(
                Level::Info,
                "dot.alternative.no_other_repos",
                &format!(
                    "   No other writable repositories to create alternatives.\n   \
                    Add another repo with {} to enable alternatives.",
                    "ins dot repo clone <url>".cyan()
                ),
                None,
            );
        } else {
            // Show available destinations
            emit(
                Level::Info,
                "dot.alternative.can_add",
                &format!(
                    "\n   {} To create an alternative, add it to another location:",
                    char::from(NerdFont::Info)
                ),
                None,
            );

            emit(
                Level::Info,
                "dot.alternative.destination",
                &format!(
                    "   {} {}",
                    char::from(NerdFont::ArrowRight),
                    format!("ins dot alternative {} --create", display_path).dimmed()
                ),
                None,
            );

            // List available destinations
            let dest_names: Vec<String> = other_destinations
                .iter()
                .take(5)
                .map(|d| format!("{} / {}", d.repo_name, d.subdir_name))
                .collect();

            let remaining = other_destinations.len().saturating_sub(5);
            let suffix = if remaining > 0 {
                format!(" (+{} more)", remaining)
            } else {
                String::new()
            };

            emit(
                Level::Info,
                "dot.alternative.available_destinations",
                &format!(
                    "   Available: {}{}",
                    dest_names.join(", ").dimmed(),
                    suffix.dimmed()
                ),
                None,
            );
        }

        return Ok(());
    }

    // Load existing overrides to mark current selection
    let overrides = OverrideConfig::load()?;
    let current_override = overrides.get_override(target_path);

    // The default source (without override) is the last one in the list (highest priority wins)
    let default_source = sources.last().cloned();

    // Build selection items
    let source_items: Vec<SourceSelectItem> = sources
        .into_iter()
        .map(|source| {
            let is_current = current_override
                .map(|o| o.source_repo == source.repo_name && o.source_subdir == source.subdir_name)
                .unwrap_or(false);
            SourceSelectItem {
                source,
                is_current,
                exists: true,
            }
        })
        .collect();

    // Check if current file is modified by user BEFORE showing picker
    if !ensure_safe_to_switch(target_path, &source_items)? {
        emit(
            Level::Error,
            "dot.alternative.modified",
            &format!(
                "{} Cannot switch source for {} - file has been modified.\n  Use 'ins dot reset {}' to discard changes first.",
                char::from(NerdFont::CrossCircle),
                display_path.yellow(),
                display_path
            ),
            None,
        );
        return Ok(());
    }

    // Build menu items
    let mut menu_items: Vec<AlternativeMenuItem> = source_items
        .into_iter()
        .map(AlternativeMenuItem::Source)
        .collect();

    // Add "Remove Override" option if there's an active override
    if current_override.is_some() {
        if let Some(default) = default_source {
            menu_items.push(AlternativeMenuItem::RemoveOverride {
                default_source: default,
            });
        }
    }

    // Show picker
    let prompt = format!("Select source for {}: ", display_path);
    match FzfWrapper::builder()
        .prompt(prompt)
        .args(fzf_mocha_args())
        .responsive_layout()
        .select(menu_items)?
    {
        FzfResult::Selected(AlternativeMenuItem::Source(item)) => {
            apply_alternative_selection(config, target_path, display_path, &item)?;
        }
        FzfResult::Selected(AlternativeMenuItem::RemoveOverride { default_source }) => {
            apply_remove_override(config, target_path, display_path, &default_source)?;
        }
        FzfResult::Cancelled => {
            emit(
                Level::Info,
                "dot.alternative.cancelled",
                &format!("{} Selection cancelled", char::from(NerdFont::Info)),
                None,
            );
        }
        FzfResult::Error(e) => {
            return Err(anyhow::anyhow!("Selection error: {}", e));
        }
        _ => {}
    }

    Ok(())
}

/// Apply the removal of an override and switch to default source
fn apply_remove_override(
    config: &Config,
    target_path: &Path,
    display_path: &str,
    default_source: &DotfileSource,
) -> Result<()> {
    let db = crate::dot::db::Database::new(config.database_path().to_path_buf())?;
    let mut overrides = OverrideConfig::load()?;

    // Remove the override
    if !overrides.remove_override(target_path)? {
        emit(
            Level::Info,
            "dot.alternative.no_override",
            &format!(
                "{} No override exists for {}",
                char::from(NerdFont::Info),
                display_path.cyan()
            ),
            None,
        );
        return Ok(());
    }

    // Apply the default source
    let new_dotfile = crate::dot::Dotfile {
        source_path: default_source.source_path.clone(),
        target_path: target_path.to_path_buf(),
    };
    new_dotfile.reset(&db)?;

    emit(
        Level::Success,
        "dot.alternative.reset",
        &format!(
            "{} Removed override for {} → now sourced from {} / {}",
            char::from(NerdFont::Check),
            display_path.cyan(),
            default_source.repo_name.green(),
            default_source.subdir_name.green()
        ),
        Some(serde_json::json!({
            "target": display_path,
            "action": "reset",
            "new_source": {
                "repo": default_source.repo_name,
                "subdir": default_source.subdir_name
            }
        })),
    );

    Ok(())
}

/// Handle the alternative command
pub fn handle_alternative(
    config: &Config,
    path: &str,
    reset: bool,
    create: bool,
    list: bool,
) -> Result<()> {
    let target_path = resolve_dotfile_path(path)?;
    let home = PathBuf::from(shellexpand::tilde("~").to_string());
    let display_path = target_path
        .strip_prefix(&home)
        .map(|p| format!("~/{}", p.display()))
        .unwrap_or_else(|_| target_path.display().to_string());

    // Check if target is a directory - if so, browse for dotfiles with alternatives
    if target_path.is_dir() {
        if reset || create {
            return Err(anyhow::anyhow!(
                "The --reset and --create flags are not supported for directories.\n\
                 Use them with a specific file path instead."
            ));
        }
        if list {
            return handle_directory_list(config, &target_path);
        }
        return handle_directory_alternatives(config, &target_path);
    }

    // Handle reset flag
    if reset {
        return handle_reset(&target_path, &display_path);
    }

    // Find all sources for this file
    let sources = find_all_sources(config, &target_path)?;

    if list {
        return handle_list(&target_path, &display_path, &sources);
    }

    if create {
        return handle_create(config, &target_path, &display_path, &sources);
    }

    // Delegate to the file handler
    handle_file_alternative(config, &target_path, &display_path)
}

/// Handle --list flag
fn handle_list(target_path: &Path, display_path: &str, sources: &[DotfileSource]) -> Result<()> {
    if sources.is_empty() {
        emit(
            Level::Info,
            "dot.alternative.list.empty",
            &format!(
                "{} No alternatives found for {}",
                char::from(NerdFont::Info),
                display_path.cyan()
            ),
            None,
        );
        return Ok(());
    }

    // Load overrides to identify current
    let overrides = OverrideConfig::load()?;
    let current_override = overrides.get_override(target_path);
    let last_source_index = sources.len() - 1;

    emit(
        Level::Info,
        "dot.alternative.list.header",
        &format!(
            "{} Alternatives for {}:",
            char::from(NerdFont::List),
            display_path.cyan()
        ),
        None,
    );

    for (i, source) in sources.iter().enumerate() {
        let is_override = current_override
            .map(|o| o.source_repo == source.repo_name && o.source_subdir == source.subdir_name)
            .unwrap_or(false);

        let is_default = current_override.is_none() && i == last_source_index;

        let status = if is_override {
            " (current override)".yellow().to_string()
        } else if is_default {
            " (current default)".dimmed().to_string()
        } else {
            "".to_string()
        };

        emit(
            Level::Info,
            "dot.alternative.list.item",
            &format!(
                "  • {} / {}{}",
                source.repo_name.green(),
                source.subdir_name.green(),
                status
            ),
            None,
        );
    }
    Ok(())
}

/// Verify if it's safe to switch the dotfile source (target not modified by user)
fn ensure_safe_to_switch(target_path: &Path, items: &[SourceSelectItem]) -> Result<bool> {
    if !target_path.exists() {
        return Ok(true);
    }

    // Check if current file is modified by user
    // If target matches ANY source, it's safe to switch (came from a repo)
    // We don't need the DB here because we're comparing content hashes directly
    // against all potential sources
    let target_hash = crate::dot::dotfile::Dotfile::compute_hash(target_path)?;

    for item in items {
        if let Ok(source_hash) =
            crate::dot::dotfile::Dotfile::compute_hash(&item.source.source_path)
            && target_hash == source_hash
        {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Apply the selected alternative
fn apply_alternative_selection(
    config: &Config,
    target_path: &Path,
    display_path: &str,
    item: &SourceSelectItem,
) -> Result<()> {
    let db = crate::dot::db::Database::new(config.database_path().to_path_buf())?;

    // Set the override
    let mut overrides = OverrideConfig::load()?;

    // Use reset() to force-apply the new source version since we already
    // verified the file is safe to switch (matches a known source)
    let new_dotfile = crate::dot::Dotfile {
        source_path: item.source.source_path.clone(),
        target_path: target_path.to_path_buf(),
    };
    new_dotfile.reset(&db)?;

    // Only save override after successful apply
    overrides.set_override(
        target_path.to_path_buf(),
        item.source.repo_name.clone(),
        item.source.subdir_name.clone(),
    )?;

    emit(
        Level::Success,
        "dot.alternative.set",
        &format!(
            "{} {} now sourced from {} / {} (applied)",
            char::from(NerdFont::Check),
            display_path.cyan(),
            item.source.repo_name.green(),
            item.source.subdir_name.green()
        ),
        Some(serde_json::json!({
            "target": display_path,
            "repo": item.source.repo_name,
            "subdir": item.source.subdir_name
        })),
    );

    Ok(())
}

/// Handle --create flag: add file to a chosen repo/subdir
fn handle_create(
    config: &Config,
    target_path: &PathBuf,
    display_path: &str,
    existing_sources: &[DotfileSource],
) -> Result<()> {
    use crate::dot::db::Database;

    // Get all available repos/subdirs
    let all_destinations = get_all_destinations(config)?;

    if all_destinations.is_empty() {
        emit(
            Level::Warn,
            "dot.alternative.no_repos",
            &format!(
                "{} No writable repositories configured",
                char::from(NerdFont::Warning)
            ),
            None,
        );
        return Ok(());
    }

    // Build selection items, marking which ones already have the file
    let items: Vec<SourceSelectItem> = all_destinations
        .into_iter()
        .map(|dest| {
            let exists = existing_sources
                .iter()
                .any(|s| s.repo_name == dest.repo_name && s.subdir_name == dest.subdir_name);
            SourceSelectItem {
                source: dest,
                is_current: false,
                exists,
            }
        })
        .collect();

    let prompt = format!("Select destination for {}: ", display_path);
    match FzfWrapper::builder().prompt(prompt).select(items)? {
        FzfResult::Selected(item) => {
            if item.exists {
                // Just set the override
                let mut overrides = OverrideConfig::load()?;
                overrides.set_override(
                    target_path.clone(),
                    item.source.repo_name.clone(),
                    item.source.subdir_name.clone(),
                )?;

                emit(
                    Level::Success,
                    "dot.alternative.set",
                    &format!(
                        "{} {} will now be sourced from {} / {}",
                        char::from(NerdFont::Check),
                        display_path.cyan(),
                        item.source.repo_name.green(),
                        item.source.subdir_name.green()
                    ),
                    None,
                );
            } else {
                // Use add_to_destination which handles file copying and DB registration
                let db = Database::new(config.database_path().to_path_buf())?;
                add_to_destination(config, &db, target_path, &item.source)?;

                // Set override to use this new source
                let mut overrides = OverrideConfig::load()?;
                overrides.set_override(
                    target_path.clone(),
                    item.source.repo_name.clone(),
                    item.source.subdir_name.clone(),
                )?;
            }
        }
        FzfResult::Cancelled => {
            emit(
                Level::Info,
                "dot.alternative.cancelled",
                &format!("{} Selection cancelled", char::from(NerdFont::Info)),
                None,
            );
        }
        FzfResult::Error(e) => {
            return Err(anyhow::anyhow!("Selection error: {}", e));
        }
        _ => {}
    }

    Ok(())
}

/// Get all available repo/subdir destinations (exported for use by add command)
pub fn get_all_destinations(config: &Config) -> Result<Vec<DotfileSource>> {
    let mut destinations = Vec::new();

    for repo_config in &config.repos {
        if !repo_config.enabled || repo_config.read_only {
            continue;
        }

        for subdir in &repo_config.active_subdirectories {
            destinations.push(DotfileSource {
                repo_name: repo_config.name.clone(),
                subdir_name: subdir.clone(),
                source_path: config.repos_path().join(&repo_config.name).join(subdir),
            });
        }
    }

    Ok(destinations)
}

/// Add a file to a specific destination (shared by both alternative and add commands)
pub fn add_to_destination(
    config: &Config,
    db: &crate::dot::db::Database,
    target_path: &Path,
    dest: &DotfileSource,
) -> Result<()> {
    use crate::dot::dotfile::Dotfile;
    use std::fs;

    let home = PathBuf::from(shellexpand::tilde("~").to_string());
    let relative = target_path.strip_prefix(&home).unwrap_or(target_path);
    let dest_path = dest.source_path.join(relative);

    // Ensure parent directory exists
    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Use Dotfile to copy and register in DB
    let dotfile = Dotfile {
        source_path: dest_path.clone(),
        target_path: target_path.to_path_buf(),
    };
    dotfile.create_source_from_target(db)?;

    // Automatically stage the new file
    let repo_path = config.repos_path().join(&dest.repo_name);
    if let Err(e) = crate::dot::git::repo_ops::git_add(&repo_path, &dest_path, false) {
        eprintln!(
            "{} Failed to stage file: {}",
            char::from(NerdFont::Warning).to_string().yellow(),
            e
        );
    }

    let relative_display = relative.display().to_string();
    emit(
        Level::Success,
        "dot.add.created",
        &format!(
            "{} Added ~/{} to {} / {}",
            char::from(NerdFont::Check),
            relative_display.green(),
            dest.repo_name.green(),
            dest.subdir_name.green()
        ),
        None,
    );

    Ok(())
}

/// Handle the --reset flag
fn handle_reset(target_path: &Path, display_path: &str) -> Result<()> {
    let mut overrides = OverrideConfig::load()?;

    if overrides.remove_override(target_path)? {
        emit(
            Level::Success,
            "dot.alternative.reset",
            &format!(
                "{} Removed override for {} (now using default priority)",
                char::from(NerdFont::Check),
                display_path.cyan()
            ),
            Some(serde_json::json!({
                "target": display_path,
                "action": "reset"
            })),
        );
    } else {
        emit(
            Level::Info,
            "dot.alternative.no_override",
            &format!(
                "{} No override exists for {}",
                char::from(NerdFont::Info),
                display_path.cyan()
            ),
            None,
        );
    }

    Ok(())
}
