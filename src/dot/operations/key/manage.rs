use anyhow::{Context, Result};
use colored::Colorize;
use std::fs;
use std::str::FromStr;

use crate::dot::config::DotfileConfig;
use crate::dot::dotfilerepo::DotfileRepo;
use crate::ui::prelude::*;

use super::util::{identities_dir, validate_key_name};

/// Rename a local key file.
pub fn handle_rename(old_name: &str, new_name: &str) -> Result<()> {
    validate_key_name(new_name)?;

    let dir = identities_dir()?;
    let old_path = dir.join(old_name);
    let new_path = dir.join(new_name);

    if !old_path.exists() {
        anyhow::bail!("Key '{}' not found in {}", old_name, dir.display());
    }

    if new_path.exists() {
        anyhow::bail!(
            "A key named '{}' already exists in {}",
            new_name,
            dir.display()
        );
    }

    fs::rename(&old_path, &new_path).with_context(|| {
        format!(
            "renaming key from '{}' to '{}'",
            old_path.display(),
            new_path.display()
        )
    })?;

    emit(
        Level::Success,
        "dot.key.rename.success",
        &format!(
            "{} Renamed key '{}' → '{}'",
            char::from(NerdFont::Check),
            old_name.cyan(),
            new_name.cyan()
        ),
        None,
    );

    Ok(())
}

/// Remove a local key file, warning if it's in use.
pub fn handle_remove(config: &DotfileConfig, name: &str) -> Result<()> {
    let dir = identities_dir()?;
    let key_path = dir.join(name);

    if !key_path.exists() {
        anyhow::bail!("Key '{}' not found in {}", name, dir.display());
    }

    // Read public key to check repo usage
    let mut public_key_opt = None;
    if let Ok(content) = fs::read_to_string(&key_path) {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("AGE-SECRET-KEY-1")
                && let Ok(identity) = age::x25519::Identity::from_str(trimmed)
            {
                public_key_opt = Some(identity.to_public().to_string());
                break;
            }
        }
    }

    if let Some(pk) = &public_key_opt {
        let repos_using_key = find_repos_using_key(config, pk);

        if !repos_using_key.is_empty() {
            emit(
                Level::Warn,
                "dot.key.remove.in_use",
                &format!(
                    "{} Key '{}' is authorized in: {}.\n{} Removing it will prevent decryption of those repositories until a different key is authorized.",
                    char::from(NerdFont::Warning).to_string().yellow(),
                    name.cyan(),
                    repos_using_key.join(", ").yellow(),
                    char::from(NerdFont::Warning).to_string().yellow()
                ),
                None,
            );
        }
    }

    fs::remove_file(&key_path)
        .with_context(|| format!("Failed to delete key file at {}", key_path.display()))?;

    emit(
        Level::Success,
        "dot.key.remove.success",
        &format!(
            "{} Removed key '{}' ({})",
            char::from(NerdFont::Check),
            name.cyan(),
            key_path.display()
        ),
        None,
    );

    Ok(())
}

/// Check which repos a given public key is authorized in.
pub fn find_repos_using_key(config: &DotfileConfig, public_key: &str) -> Vec<String> {
    let writable_repos = config.get_writable_repos();
    writable_repos
        .iter()
        .filter_map(|r| {
            let dotfile_repo = DotfileRepo::new(config, r.name.clone()).ok()?;
            let repo_path = dotfile_repo.local_path(config).ok()?;
            let meta = crate::dot::meta::read_meta(&repo_path).ok()?;
            if meta
                .encryption_recipients
                .iter()
                .any(|rec| rec == public_key)
            {
                Some(r.name.clone())
            } else {
                None
            }
        })
        .collect()
}
