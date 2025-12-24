//! Alternative source selection for dotfiles
//!
//! Allows users to interactively select which repository/subdirectory
//! a dotfile should be sourced from.

use anyhow::Result;
use colored::Colorize;
use std::path::PathBuf;

use crate::dot::config::Config;
use crate::dot::override_config::{find_all_sources, DotfileSource, OverrideConfig};
use crate::dot::utils::resolve_dotfile_path;
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper};
use crate::ui::prelude::*;

/// Wrapper for DotfileSource to implement FzfSelectable
#[derive(Clone)]
struct SourceSelectItem {
    source: DotfileSource,
    is_current: bool,
}

impl FzfSelectable for SourceSelectItem {
    fn fzf_display_text(&self) -> String {
        let indicator = if self.is_current { " (current)" } else { "" };
        format!(
            "{} / {}{}",
            self.source.repo_name, self.source.subdir_name, indicator
        )
    }

    fn fzf_key(&self) -> String {
        format!("{}:{}", self.source.repo_name, self.source.subdir_name)
    }
}

/// Handle the alternative command
pub fn handle_alternative(config: &Config, path: &str, reset: bool) -> Result<()> {
    let target_path = resolve_dotfile_path(path)?;
    let home = PathBuf::from(shellexpand::tilde("~").to_string());
    let display_path = target_path
        .strip_prefix(&home)
        .map(|p| format!("~/{}", p.display()))
        .unwrap_or_else(|_| target_path.display().to_string());

    // Handle reset flag
    if reset {
        return handle_reset(&target_path, &display_path);
    }

    // Find all sources for this file
    let sources = find_all_sources(config, &target_path)?;

    if sources.is_empty() {
        emit(
            Level::Warn,
            "dot.alternative.not_found",
            &format!(
                "{} No dotfile sources found for {}",
                char::from(NerdFont::Warning),
                display_path.yellow()
            ),
            None,
        );
        return Ok(());
    }

    if sources.len() == 1 {
        let source = &sources[0];
        emit(
            Level::Info,
            "dot.alternative.single_source",
            &format!(
                "{} {} is only available from {} / {}",
                char::from(NerdFont::Info),
                display_path.cyan(),
                source.repo_name.green(),
                source.subdir_name.green()
            ),
            None,
        );
        return Ok(());
    }

    // Load existing overrides to mark current selection
    let overrides = OverrideConfig::load()?;
    let current_override = overrides.get_override(&target_path);

    // Build selection items
    let items: Vec<SourceSelectItem> = sources
        .into_iter()
        .map(|source| {
            let is_current = current_override
                .map(|o| o.source_repo == source.repo_name && o.source_subdir == source.subdir_name)
                .unwrap_or(false);
            SourceSelectItem { source, is_current }
        })
        .collect();

    // Show picker
    let prompt = format!("Select source for {}: ", display_path);
    match FzfWrapper::builder().prompt(prompt).select(items)? {
        FzfResult::Selected(item) => {
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
                Some(serde_json::json!({
                    "target": display_path,
                    "repo": item.source.repo_name,
                    "subdir": item.source.subdir_name
                })),
            );
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

/// Handle the --reset flag
fn handle_reset(target_path: &PathBuf, display_path: &str) -> Result<()> {
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
