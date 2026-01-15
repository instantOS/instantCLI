//! Apply alternative selections - set overrides and copy files.

use std::fs;
use std::path::Path;

use anyhow::Result;
use colored::Colorize;

use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::dotfile::Dotfile;
use crate::dot::override_config::{DotfileSource, OverrideConfig};
use crate::ui::prelude::*;

use super::discovery::home_dir;
use super::picker::SourceOption;

/// Check if target file is safe to switch (matches any known source).
pub fn is_safe_to_switch(target_path: &Path, sources: &[SourceOption]) -> Result<bool> {
    if !target_path.exists() {
        return Ok(true);
    }

    let target_hash = Dotfile::compute_hash(target_path)?;
    for item in sources {
        if let Ok(source_hash) = Dotfile::compute_hash(&item.source.source_path)
            && target_hash == source_hash
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Set override and apply the source file.
pub fn set_alternative(
    config: &Config,
    target_path: &Path,
    display_path: &str,
    source: &SourceOption,
) -> Result<()> {
    let db = Database::new(config.database_path().to_path_buf())?;
    let mut overrides = OverrideConfig::load()?;

    let dotfile = Dotfile {
        source_path: source.source.source_path.clone(),
        target_path: target_path.to_path_buf(),
    };
    dotfile.reset(&db)?;

    overrides.set_override(
        target_path.to_path_buf(),
        source.source.repo_name.clone(),
        source.source.subdir_name.clone(),
    )?;

    emit(
        Level::Success,
        "dot.alternative.set",
        &format!(
            "{} {} now sourced from {} / {}",
            char::from(NerdFont::Check),
            display_path.cyan(),
            source.source.repo_name.green(),
            source.source.subdir_name.green()
        ),
        Some(serde_json::json!({
            "target": display_path,
            "repo": source.source.repo_name,
            "subdir": source.source.subdir_name
        })),
    );
    Ok(())
}

/// Remove override and revert to default source.
pub fn remove_override(
    config: &Config,
    target_path: &Path,
    display_path: &str,
    default_source: &DotfileSource,
) -> Result<()> {
    let db = Database::new(config.database_path().to_path_buf())?;
    let mut overrides = OverrideConfig::load()?;

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

    let dotfile = Dotfile {
        source_path: default_source.source_path.clone(),
        target_path: target_path.to_path_buf(),
    };
    dotfile.reset(&db)?;

    emit(
        Level::Success,
        "dot.alternative.reset",
        &format!(
            "{} Removed override for {} -> {} / {}",
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

/// Reset override for a path (CLI --reset flag).
pub fn reset_override(target_path: &Path, display_path: &str) -> Result<()> {
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

/// Add a file to a destination repo (copy + register + stage).
pub fn add_to_destination(
    config: &Config,
    db: &Database,
    target_path: &Path,
    dest: &DotfileSource,
) -> Result<()> {
    let relative = target_path.strip_prefix(home_dir()).unwrap_or(target_path);
    let dest_path = dest.source_path.join(relative);

    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let dotfile = Dotfile {
        source_path: dest_path.clone(),
        target_path: target_path.to_path_buf(),
    };
    dotfile.create_source_from_target(db)?;

    let repo_path = config.repos_path().join(&dest.repo_name);
    if let Err(e) = crate::dot::git::repo_ops::git_add(&repo_path, &dest_path, false) {
        eprintln!(
            "{} Failed to stage file: {}",
            char::from(NerdFont::Warning).to_string().yellow(),
            e
        );
    }

    emit(
        Level::Success,
        "dot.add.created",
        &format!(
            "{} Added ~/{} to {} / {}",
            char::from(NerdFont::Check),
            relative.display().to_string().green(),
            dest.repo_name.green(),
            dest.subdir_name.green()
        ),
        None,
    );
    Ok(())
}
