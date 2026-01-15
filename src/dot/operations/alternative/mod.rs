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
use picker::{MenuItem, SourceOption};

// Re-export for external use (add command uses these)
pub use apply::add_to_destination;
pub use discovery::get_destinations as get_all_destinations;

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

    let dotfiles = discover_dotfiles(config, dir, filter)?;

    if dotfiles.is_empty() {
        let msg = if create_mode {
            format!("No dotfiles found in {}", display.cyan())
        } else {
            format!("No dotfiles with alternatives in {}", display.cyan())
        };
        emit(Level::Info, "dot.alternative.empty", &msg, None);
        return Ok(());
    }

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

    let selected = match FzfWrapper::builder()
        .prompt(format!("Select dotfile in {}: ", display))
        .args(fzf_mocha_args())
        .responsive_layout()
        .select(dotfiles)?
    {
        FzfResult::Selected(item) => item,
        FzfResult::Cancelled => {
            emit_cancelled();
            return Ok(());
        }
        FzfResult::Error(e) => return Err(anyhow::anyhow!("Selection error: {}", e)),
        _ => return Ok(()),
    };

    if create_mode {
        let sources = find_all_sources(config, &selected.target_path)?;
        create_flow(
            config,
            &selected.target_path,
            &selected.display_path,
            &sources,
        )
    } else {
        select_flow(config, &selected.target_path, &selected.display_path, true)
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

    if current.is_some() {
        if let Some(default) = default_source {
            menu.push(MenuItem::RemoveOverride {
                default_source: default,
            });
        }
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
    config: &Config,
    path: &Path,
    display: &str,
    existing: &[DotfileSource],
) -> Result<()> {
    let destinations = get_destinations(config);

    if destinations.is_empty() {
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

    let items: Vec<SourceOption> = destinations
        .into_iter()
        .map(|dest| {
            let exists = existing
                .iter()
                .any(|s| s.repo_name == dest.repo_name && s.subdir_name == dest.subdir_name);
            SourceOption {
                source: dest,
                is_current: false,
                exists,
            }
        })
        .collect();

    match FzfWrapper::builder()
        .prompt(format!("Select destination for {}: ", display))
        .select(items)?
    {
        FzfResult::Selected(item) => {
            if item.exists {
                // Already exists, just set override
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
                // Copy file to destination
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
        FzfResult::Cancelled => {
            emit_cancelled();
            Ok(())
        }
        FzfResult::Error(e) => Err(anyhow::anyhow!("Selection error: {}", e)),
        _ => Ok(()),
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
