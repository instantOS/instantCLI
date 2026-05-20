use anyhow::Result;
use colored::Colorize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::dot::config::DotfileConfig;
use crate::ui::prelude::*;

/// Get the identities directory path, creating it if needed.
pub fn identities_dir() -> Result<PathBuf> {
    let config_dir = crate::common::paths::instant_config_dir()?;
    let dir = config_dir.join("encryption").join("identities");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Validate a key name: non-empty, no path separators, no `.`/`..`.
pub fn validate_key_name(name: &str) -> Result<()> {
    if name.is_empty() {
        anyhow::bail!("Key name cannot be empty");
    }
    if name.contains('/') || name.contains(std::path::MAIN_SEPARATOR) {
        anyhow::bail!("Key name cannot contain path separators");
    }
    if name == "." || name == ".." {
        anyhow::bail!("Key name cannot be '.' or '..'");
    }
    Ok(())
}

/// Recursively find `.age` files under a directory.
pub fn find_age_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if dir.is_dir() {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                find_age_files(&path, files)?;
            } else if path.is_file() && crate::dot::encryption::is_encrypted_source(&path) {
                files.push(path);
            }
        }
    }
    Ok(())
}

/// Auto-select a writable repo when none is explicitly given.
/// Emits a warning if multiple writable repos exist.
/// `action_verb` is the present-participle verb for user-facing messages (e.g. "authorizing").
pub fn resolve_writable_repo(
    config: &DotfileConfig,
    repo_name_opt: Option<&str>,
    action_verb: &str,
    event_tag: &str,
) -> Result<(String, bool)> {
    if let Some(name) = repo_name_opt {
        return Ok((name.to_string(), false));
    }
    let writable_repos = config.get_writable_repos();
    if writable_repos.is_empty() {
        anyhow::bail!(
            "No writable repositories found in config to {} keys.",
            action_verb
        );
    }
    let chosen = writable_repos[0].name.clone();
    if writable_repos.len() > 1 {
        let other_names: Vec<&str> = writable_repos
            .iter()
            .skip(1)
            .map(|r| r.name.as_str())
            .collect();
        emit(
            Level::Warn,
            event_tag,
            &format!(
                "{} No --repo given; {} in '{}' (other writable repos: {}). Pass --repo to choose explicitly.",
                char::from(NerdFont::Warning),
                action_verb,
                chosen.cyan(),
                other_names.join(", "),
            ),
            Some(serde_json::json!({
                "selected_repo": chosen,
                "other_writable_repos": other_names,
            })),
        );
    }
    Ok((chosen, true))
}
