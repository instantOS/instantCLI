//! Listing helpers for alternatives.

use std::path::Path;

use anyhow::Result;
use colored::Colorize;

use crate::dot::config::Config;
use crate::dot::override_config::{DotfileSource, OverrideConfig};
use crate::ui::prelude::*;

use super::discovery::{DiscoveryFilter, discover_dotfiles};

pub(crate) fn list_directory(config: &Config, dir: &Path, display: &str) -> Result<()> {
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

pub(crate) fn list_file(path: &Path, display: &str, sources: &[DotfileSource]) -> Result<()> {
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
