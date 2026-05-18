use crate::common::home_dir;
use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use crate::dot::dotfile::{Dotfile, SourceKind};
use crate::dot::encryption::{EncryptedFailureReason, classify_encrypted_failure};
use crate::dot::units::{get_all_units, get_modified_units};
use crate::dot::utils::get_all_dotfiles;
use crate::ui::prelude::*;
use anyhow::Result;
use colored::Colorize;
use std::collections::HashSet;
use std::path::PathBuf;

/// Result of applying a single dotfile
#[derive(Debug, PartialEq)]
enum ApplyAction {
    Created,
    Updated,
    Skipped,
    SkippedEncrypted(EncryptedFailureReason),
    SkippedUnit,
    AlreadyUpToDate,
}

/// Statistics collected during apply operation
#[derive(Default)]
struct ApplyStats {
    created: Vec<String>,
    updated: Vec<String>,
    skipped: Vec<String>,
    skipped_encrypted: Vec<String>,
    skipped_unit_files: usize,
    unchanged: usize,
    reported_units: HashSet<PathBuf>,
}

impl ApplyStats {
    fn skipped_units(&self) -> usize {
        self.reported_units.len()
    }
}

/// Apply all dotfiles from configured repositories
pub fn apply_all(
    config: &DotfileConfig,
    db: &Database,
    include_root: bool,
    root_only: bool,
) -> Result<()> {
    let all_dotfiles = get_all_dotfiles(config, db, include_root || root_only)?;

    if all_dotfiles.is_empty() {
        emit(
            Level::Info,
            "dot.apply.no_dotfiles",
            &format!("{} No dotfiles configured", char::from(NerdFont::Info)),
            None,
        );
        return Ok(());
    }

    let home_dotfiles: Vec<_> = all_dotfiles.values().filter(|d| !d.is_root).collect();
    let root_dotfiles: Vec<_> = all_dotfiles.values().filter(|d| d.is_root).collect();

    let mut stats = ApplyStats::default();

    if !root_only {
        for dotfile in &home_dotfiles {
            let action = determine_and_apply_action(
                dotfile,
                &get_all_units(config, db)?,
                &get_modified_units(&all_dotfiles, &get_all_units(config, db)?, db)?,
                &mut stats,
                db,
            )?;
            emit_action_result(&action, dotfile);
            record_action(&action, dotfile, &mut stats);
        }
    }

    if !root_dotfiles.is_empty() && (include_root || root_only) {
        if root_only {
            for dotfile in &root_dotfiles {
                let action = determine_and_apply_action(
                    dotfile,
                    &get_all_units(config, db)?,
                    &get_modified_units(&all_dotfiles, &get_all_units(config, db)?, db)?,
                    &mut stats,
                    db,
                )?;
                emit_action_result(&action, dotfile);
                record_action(&action, dotfile, &mut stats);
            }
        } else {
            let home_dir = home_dir();
            let home_dir_str = home_dir.to_string_lossy();
            emit(
                Level::Info,
                "dot.apply.root_files",
                &format!(
                    "{} Applying {} root dotfile(s) (requires sudo)",
                    char::from(NerdFont::ShieldCheck),
                    root_dotfiles.len()
                ),
                None,
            );

            let status = std::process::Command::new("sudo")
                .arg("ins")
                .arg("dot")
                .arg("apply")
                .arg("--root-only")
                .arg("--home")
                .arg(home_dir_str.as_ref())
                .status();

            if let Err(e) = status {
                emit(
                    Level::Warn,
                    "dot.apply.root_failed",
                    &format!(
                        "{} Failed to spawn sudo for root dotfiles: {}",
                        char::from(NerdFont::Warning),
                        e
                    ),
                    None,
                );
            } else if let Ok(s) = status {
                if !s.success() {
                    emit(
                        Level::Warn,
                        "dot.apply.root_failed",
                        &format!(
                            "{} Applying root dotfiles failed or was cancelled",
                            char::from(NerdFont::Warning)
                        ),
                        None,
                    );
                }
            }
        }
    }

    db.cleanup_hashes(config.hash_cleanup_days)?;
    if !root_only || (root_only && !root_dotfiles.is_empty()) {
        print_apply_summary(&stats);
    }

    Ok(())
}

/// Determine and execute the appropriate action for a dotfile
fn determine_and_apply_action(
    dotfile: &Dotfile,
    units: &[PathBuf],
    modified_units: &HashSet<PathBuf>,
    stats: &mut ApplyStats,
    db: &Database,
) -> Result<ApplyAction> {
    // Check if file belongs to a modified unit
    let unit_paths = crate::dot::units::find_units_for_path(&dotfile.target_path, units);
    if !unit_paths.is_empty() {
        let mut should_skip = false;
        for unit_path in unit_paths {
            if !modified_units.contains(&unit_path) {
                continue;
            }

            should_skip = true;

            // Report the unit skip once per unit
            if !stats.reported_units.contains(&unit_path) {
                emit(
                    Level::Warn,
                    "dot.apply.skipped_unit",
                    &format!(
                        "{} Skipped unit ~/{} (contains modified files)",
                        char::from(NerdFont::ShieldAlert),
                        unit_path.display().to_string().yellow()
                    ),
                    Some(serde_json::json!({
                        "unit": format!("~/{}", unit_path.display()),
                        "action": "skipped_unit",
                        "reason": "unit_modified"
                    })),
                );
                stats.reported_units.insert(unit_path);
            }
        }

        if should_skip {
            return Ok(ApplyAction::SkippedUnit);
        }
    }

    apply_single_dotfile(dotfile, db)
}

/// Apply a single dotfile and determine what action was taken
fn apply_single_dotfile(dotfile: &Dotfile, db: &Database) -> Result<ApplyAction> {
    let target_exists = dotfile.target_path.exists();
    let is_modified = match dotfile.is_target_unmodified(db) {
        Ok(unmodified) => !unmodified,
        Err(err) if dotfile.kind == SourceKind::Age => {
            if target_exists {
                return Ok(ApplyAction::SkippedEncrypted(classify_encrypted_failure(
                    &err,
                )));
            }
            return Err(err);
        }
        Err(err) => return Err(err),
    };
    let is_outdated = match dotfile.is_outdated(db) {
        Ok(outdated) => outdated,
        Err(err) if dotfile.kind == SourceKind::Age => {
            return Ok(ApplyAction::SkippedEncrypted(classify_encrypted_failure(
                &err,
            )));
        }
        Err(err) => return Err(err),
    };

    if is_modified {
        return Ok(ApplyAction::Skipped);
    }

    if !is_outdated {
        let _ = dotfile.get_file_hash(&dotfile.source_path, true, db);
        return Ok(ApplyAction::AlreadyUpToDate);
    }

    if let Err(err) = dotfile.apply(db) {
        if dotfile.kind == SourceKind::Age {
            return Ok(ApplyAction::SkippedEncrypted(classify_encrypted_failure(
                &err,
            )));
        }
        return Err(err);
    }

    if !target_exists {
        Ok(ApplyAction::Created)
    } else {
        Ok(ApplyAction::Updated)
    }
}

/// Emit user-visible output for an action
fn emit_action_result(action: &ApplyAction, dotfile: &Dotfile) {
    let path_str = crate::dot::display_path(&dotfile.target_path, dotfile.is_root);

    match action {
        ApplyAction::Created => {
            emit(
                Level::Success,
                "dot.apply.created",
                &format!(
                    "{} Created: {}",
                    char::from(NerdFont::Check),
                    path_str.green()
                ),
                Some(serde_json::json!({"path": path_str, "action": "created"})),
            );
        }
        ApplyAction::Updated => {
            emit(
                Level::Success,
                "dot.apply.updated",
                &format!(
                    "{} Updated: {}",
                    char::from(NerdFont::Check),
                    path_str.green()
                ),
                Some(serde_json::json!({"path": path_str, "action": "updated"})),
            );
        }
        ApplyAction::Skipped => {
            emit(
                Level::Warn,
                "dot.apply.skipped",
                &format!(
                    "{} Skipped (user modified): {}",
                    char::from(NerdFont::ShieldAlert),
                    path_str.yellow()
                ),
                Some(
                    serde_json::json!({"path": path_str, "action": "skipped", "reason": "user_modified"}),
                ),
            );
        }
        ApplyAction::SkippedEncrypted(reason) => {
            emit(
                Level::Warn,
                "dot.apply.skipped_encrypted",
                &format!(
                    "{} Skipped (encrypted, {}): {}",
                    char::from(NerdFont::ShieldAlert),
                    reason.label(),
                    path_str.yellow()
                ),
                Some(serde_json::json!({
                    "path": path_str,
                    "action": "skipped",
                    "reason": reason.code()
                })),
            );
        }
        ApplyAction::SkippedUnit | ApplyAction::AlreadyUpToDate => {}
    }
}

/// Record action results into stats
fn record_action(action: &ApplyAction, dotfile: &Dotfile, stats: &mut ApplyStats) {
    let path_str = crate::dot::display_path(&dotfile.target_path, dotfile.is_root);

    match action {
        ApplyAction::Created => stats.created.push(path_str),
        ApplyAction::Updated => stats.updated.push(path_str),
        ApplyAction::Skipped => stats.skipped.push(path_str),
        ApplyAction::SkippedEncrypted(_) => stats.skipped_encrypted.push(path_str),
        ApplyAction::SkippedUnit => stats.skipped_unit_files += 1,
        ApplyAction::AlreadyUpToDate => stats.unchanged += 1,
    }
}

/// Print summary of apply operation
fn print_apply_summary(stats: &ApplyStats) {
    separator(false);

    let summary_title = if matches!(get_output_format(), OutputFormat::Json) {
        "Apply Summary".to_string()
    } else {
        format!(
            "{} {} Apply Summary",
            char::from(NerdFont::Chart),
            char::from(NerdFont::List)
        )
    };

    let summary_data = serde_json::json!({
        "created": stats.created.len(),
        "updated": stats.updated.len(),
        "skipped": stats.skipped.len(),
        "skipped_encrypted": stats.skipped_encrypted.len(),
        "skipped_unit_files": stats.skipped_unit_files,
        "skipped_units": stats.skipped_units(),
        "unchanged": stats.unchanged
    });

    if matches!(get_output_format(), OutputFormat::Json) {
        emit(Level::Info, "dot.apply.summary.title", &summary_title, None);
        emit(
            Level::Info,
            "dot.apply.summary",
            &format!(
                "  Created: {}\n  Updated: {}\n  Skipped: {}\n  Skipped (encrypted): {}\n  Skipped (units): {} files in {} units\n  Unchanged: {}",
                stats.created.len(),
                stats.updated.len(),
                stats.skipped.len(),
                stats.skipped_encrypted.len(),
                stats.skipped_unit_files,
                stats.skipped_units(),
                stats.unchanged
            ),
            Some(summary_data),
        );
        separator(false);
        return;
    }

    emit(
        Level::Info,
        "dot.apply.summary.title",
        &summary_title,
        Some(summary_data.clone()),
    );

    let mut entries: Vec<(Level, char, &str, String, &str)> = vec![
        (
            Level::Success,
            char::from(NerdFont::Check),
            "Created",
            stats.created.len().to_string(),
            "dot.apply.summary.created",
        ),
        (
            Level::Success,
            char::from(NerdFont::Check),
            "Updated",
            stats.updated.len().to_string(),
            "dot.apply.summary.updated",
        ),
    ];

    if !stats.skipped.is_empty() {
        entries.push((
            Level::Warn,
            char::from(NerdFont::ShieldAlert),
            "Skipped",
            stats.skipped.len().to_string(),
            "dot.apply.summary.skipped",
        ));
    }

    if !stats.skipped_encrypted.is_empty() {
        emit(
            Level::Warn,
            "dot.apply.summary.skipped_encrypted",
            &format!(
                "{} Skipped {} encrypted file(s); see warnings above for reason details",
                char::from(NerdFont::ShieldAlert),
                stats.skipped_encrypted.len()
            ),
            Some(serde_json::json!({
                "skipped_encrypted": stats.skipped_encrypted.len(),
                "reason": "encrypted_failure"
            })),
        );
    }

    if stats.skipped_units() > 0 {
        entries.push((
            Level::Warn,
            char::from(NerdFont::ShieldAlert),
            "Skipped Units",
            format!(
                "{} files in {} units",
                stats.skipped_unit_files,
                stats.skipped_units()
            ),
            "dot.apply.summary.skipped_units",
        ));
    }

    entries.push((
        Level::Info,
        char::from(NerdFont::Clock2),
        "Unchanged",
        stats.unchanged.to_string(),
        "dot.apply.summary.unchanged",
    ));

    let label_width = entries
        .iter()
        .map(|(_, _, l, _, _)| l.len())
        .max()
        .unwrap_or(0)
        + 4;

    for (level, icon, label, value, code) in entries {
        let label_with_icon = format!("{icon} {label}:");
        let message = format!("{label_with_icon:<width$} {value}", width = label_width);
        emit(
            level,
            code,
            &message,
            Some(serde_json::json!({"label": label.to_lowercase(), "count": value})),
        );
    }

    separator(false);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::TildePath;
    use crate::dot::config::{DotfileConfig, Repo};
    use crate::dot::db::Database;
    use crate::dot::types::RepoMetaData;
    use serial_test::serial;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    #[serial]
    fn apply_all_skips_undecryptable_age_file_and_applies_plain_file() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        let config_home = dir.path().join("config");
        let repos_dir = dir.path().join("repos");
        let repo_dir = repos_dir.join("test-repo");
        let dots_dir = repo_dir.join("dots");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&config_home).unwrap();
        fs::create_dir_all(&dots_dir).unwrap();
        fs::write(dots_dir.join("plain.txt"), "plain").unwrap();
        fs::write(dots_dir.join("secret.txt.age"), "not an age file").unwrap();

        let prev_home = std::env::var_os("HOME");
        let prev_xdg = std::env::var_os("XDG_CONFIG_HOME");
        let prev_age = std::env::var_os("AGE_IDENTITY");
        // SAFETY: this test is serialised and restores the process env below.
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("XDG_CONFIG_HOME", &config_home);
            std::env::remove_var("AGE_IDENTITY");
        }

        let config = DotfileConfig {
            repos: vec![Repo {
                url: "local".to_string(),
                name: "test-repo".to_string(),
                branch: None,
                active_subdirectories: Some(vec!["dots".to_string()]),
                enabled: true,
                read_only: false,
                metadata: Some(RepoMetaData {
                    name: "test-repo".to_string(),
                    dots_dirs: vec!["dots".to_string()],
                    ..RepoMetaData::default()
                }),
            }],
            repos_dir: TildePath::new(repos_dir),
            database_dir: TildePath::new(dir.path().join("test.db")),
            ..DotfileConfig::default()
        };
        let db = Database::new(config.database_path().to_path_buf()).unwrap();

        let result = apply_all(&config, &db, false, false);

        unsafe {
            match prev_home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
            match prev_xdg {
                Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
                None => std::env::remove_var("XDG_CONFIG_HOME"),
            }
            match prev_age {
                Some(v) => std::env::set_var("AGE_IDENTITY", v),
                None => std::env::remove_var("AGE_IDENTITY"),
            }
        }

        result.expect("apply should skip encrypted failures instead of aborting");
        assert_eq!(fs::read_to_string(home.join("plain.txt")).unwrap(), "plain");
        assert!(
            !home.join("secret.txt").exists(),
            "undecryptable encrypted file should be skipped"
        );
    }
}
