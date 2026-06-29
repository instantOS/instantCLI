//! Apply alternative selections - set overrides and copy files.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use colored::Colorize;

use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use crate::dot::dotfile::Dotfile;
use crate::dot::override_config::{DotfileSource, OverrideConfig};
use crate::ui::prelude::*;

use super::picker::SourceOption;
use crate::dot::sources;

fn relative_target_path(target_path: &Path) -> Result<(PathBuf, bool)> {
    if let Ok(relative) = target_path.strip_prefix(sources::home_dir()) {
        return Ok((relative.to_path_buf(), false));
    }

    let relative = target_path.strip_prefix("/").map_err(|_| {
        anyhow::anyhow!(
            "Target path must be inside the home directory or an absolute root path: {}",
            target_path.display()
        )
    })?;
    Ok((relative.to_path_buf(), true))
}

fn display_target_path(relative: &Path, is_root_target: bool) -> String {
    if is_root_target {
        format!("/{}", relative.display())
    } else {
        format!("~/{}", relative.display())
    }
}

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

    let config = DotfileConfig::load(None)?;
    let db = Database::new(config.database_path().to_path_buf())?;
    db.source_hash_exists_anywhere(&target_hash)
}

/// Set override and apply the source file.
pub fn set_alternative(
    config: &DotfileConfig,
    target_path: &Path,
    display_path: &str,
    source: &SourceOption,
) -> Result<()> {
    let db = Database::new(config.database_path().to_path_buf())?;
    let mut overrides = OverrideConfig::load()?;

    let dotfile = Dotfile::new(
        source.source.source_path.clone(),
        target_path.to_path_buf(),
        false,
    );
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
    config: &DotfileConfig,
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

    let dotfile = Dotfile::new(
        default_source.source_path.clone(),
        target_path.to_path_buf(),
        false,
    );
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
    config: &DotfileConfig,
    db: &Database,
    target_path: &Path,
    dest: &DotfileSource,
    force: bool,
    recipients: Option<&[Box<dyn age::Recipient>]>,
) -> Result<bool> {
    // Ensure the target file exists before attempting to copy
    if !target_path.exists() {
        anyhow::bail!(
            "Target file does not exist: {}\n\
            Cannot create an alternative for a file that doesn't exist.",
            target_path.display()
        );
    }

    if !force {
        if let Some(ignore_file) = crate::dot::insignore::match_home_path(target_path)? {
            println!(
                "{}",
                crate::dot::insignore::format_skip_message(target_path, &ignore_file)
            );
            return Ok(false);
        }

        let repo_root = config.repos_path().join(&dest.repo_name);
        if let Some(ignore_file) =
            crate::dot::insignore::match_repo_target_path(&repo_root, target_path)?
        {
            println!(
                "{}",
                crate::dot::insignore::format_skip_message(target_path, &ignore_file)
            );
            return Ok(false);
        }
    }

    let (relative, is_root_target) = relative_target_path(target_path)?;
    let is_root_destination = dest.subdir_name.ends_with("_root");
    if is_root_target != is_root_destination {
        anyhow::bail!(
            "Destination '{}/{}' cannot store {}.\n\
             Dotfile directories ending in '_root' store root-owned dotfiles.",
            dest.repo_name,
            dest.subdir_name,
            if is_root_target {
                "root-owned dotfiles"
            } else {
                "home dotfiles"
            }
        );
    }

    let mut dest_path = dest.source_path.join(&relative);
    if recipients.is_some() {
        dest_path = crate::dot::encryption::append_age_suffix(&dest_path);
    }

    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let dotfile = Dotfile::new(dest_path.clone(), target_path.to_path_buf(), is_root_target);
    if let Some(recs) = recipients {
        dotfile.create_encrypted_source_from_target(db, recs)?;
    } else {
        dotfile.create_source_from_target(db)?;
    }

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
            "{} Added {} to {} / {}",
            char::from(NerdFont::Check),
            display_target_path(&relative, is_root_target).green(),
            dest.repo_name.green(),
            dest.subdir_name.green()
        ),
        None,
    );
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::{add_to_destination, display_target_path, relative_target_path};
    use crate::common::TildePath;
    use crate::dot::config::DotfileConfig;
    use crate::dot::db::Database;
    use crate::dot::override_config::DotfileSource;
    use crate::dot::test_util::EnvGuard;
    use serial_test::serial;
    use std::fs;
    use std::path::Path;

    #[test]
    #[serial]
    fn root_target_paths_are_stored_relative_to_root() {
        let _home = EnvGuard::set("HOME", "/tmp/instantcli-test-home");
        let (relative, is_root) =
            relative_target_path(Path::new("/etc/ppp/ip-up.d/99-fortivpn-routes.sh")).unwrap();

        assert!(is_root);
        assert_eq!(relative, Path::new("etc/ppp/ip-up.d/99-fortivpn-routes.sh"));
        assert_eq!(
            display_target_path(&relative, is_root),
            "/etc/ppp/ip-up.d/99-fortivpn-routes.sh"
        );
    }

    #[test]
    #[serial]
    fn root_owned_dotfile_is_copied_under_root_destination() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let repos_dir = dir.path().join("repos");
        let repo_dir = repos_dir.join("personal");
        let root_dir = repo_dir.join("dots_root");
        let target = dir.path().join("system/etc/example.conf");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&root_dir).unwrap();
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, "root config").unwrap();
        std::process::Command::new("git")
            .arg("init")
            .arg("-q")
            .current_dir(&repo_dir)
            .status()
            .unwrap();

        let _home = EnvGuard::set("HOME", &home);
        let config = DotfileConfig {
            repos_dir: TildePath::new(repos_dir),
            database_dir: TildePath::new(dir.path().join("instant.db")),
            ..DotfileConfig::default()
        };
        let db = Database::new(config.database_path().to_path_buf()).unwrap();
        let dest = DotfileSource {
            repo_name: "personal".to_string(),
            subdir_name: "dots_root".to_string(),
            source_path: root_dir.clone(),
        };

        add_to_destination(&config, &db, &target, &dest, true, None).unwrap();

        let relative = target.strip_prefix("/").unwrap();
        assert_eq!(
            fs::read_to_string(root_dir.join(relative)).unwrap(),
            "root config"
        );
    }
}
