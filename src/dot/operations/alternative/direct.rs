//! Non-interactive handlers for setting/creating alternatives.

use std::path::Path;

use anyhow::Result;

use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::override_config::{OverrideConfig, find_all_sources};
use crate::ui::prelude::*;

use super::apply::{add_to_destination, is_safe_to_switch, set_alternative};
use super::discovery::get_destinations;
use super::picker::SourceOption;

/// Handle --set REPO[/SUBDIR] flag (non-interactive).
pub(crate) fn handle_set_direct(
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
    let source_option = SourceOption {
        source: (*source).clone(),
        is_current: false,
        exists: true,
    };

    // Check if safe to switch
    let all_options: Vec<_> = sources
        .into_iter()
        .map(|s| SourceOption {
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
pub(crate) fn handle_create_direct(
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
