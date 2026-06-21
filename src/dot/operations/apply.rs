use crate::common::home_dir;
use crate::dot::config::DotfileConfig;
use crate::dot::db::{Database, ManagedTarget};
use crate::dot::dotfile::{Dotfile, SourceKind};
use crate::dot::encryption::{EncryptedFailureReason, classify_encrypted_failure};
use crate::dot::units::{get_all_units, get_modified_units};
use crate::dot::utils::get_all_dotfiles;
use crate::ui::prelude::*;
use anyhow::{Context, Result};
use colored::Colorize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

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
    apply_all_with_reconciliation(config, db, include_root, root_only, true)
}

pub(crate) fn apply_all_with_reconciliation(
    config: &DotfileConfig,
    db: &Database,
    include_root: bool,
    root_only: bool,
    reconcile_removed: bool,
) -> Result<()> {
    let all_dotfiles = get_all_dotfiles(config, db, include_root || root_only)?;

    let home_dotfiles: Vec<_> = all_dotfiles.values().filter(|d| !d.is_root).collect();
    let root_dotfiles: Vec<_> = all_dotfiles.values().filter(|d| d.is_root).collect();

    let mut stats = ApplyStats::default();

    let all_units = get_all_units(config, db)?;
    let modified_units = get_modified_units(&all_dotfiles, &all_units, db)?;

    if !root_only {
        for dotfile in &home_dotfiles {
            let action =
                determine_and_apply_action(dotfile, &all_units, &modified_units, &mut stats, db)?;
            emit_action_result(&action, dotfile);
            record_action(&action, dotfile, &mut stats);
            record_managed_target_if_confirmed(config, db, dotfile, &action)?;
        }

        reconcile_removed_targets(config, db, &all_dotfiles, false, reconcile_removed)?;
    }

    if root_only {
        for dotfile in &root_dotfiles {
            let action =
                determine_and_apply_action(dotfile, &all_units, &modified_units, &mut stats, db)?;
            emit_action_result(&action, dotfile);
            record_action(&action, dotfile, &mut stats);
            record_managed_target_if_confirmed(config, db, dotfile, &action)?;
        }
        reconcile_removed_targets(config, db, &all_dotfiles, true, reconcile_removed)?;
    } else if should_delegate_root_apply(include_root, root_dotfiles.len()) {
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
        } else if let Ok(s) = status
            && !s.success()
        {
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

    db.cleanup_hashes(config.hash_cleanup_days)?;
    if all_dotfiles.is_empty() {
        emit(
            Level::Info,
            "dot.apply.no_dotfiles",
            &format!("{} No dotfiles configured", char::from(NerdFont::Info)),
            None,
        );
    } else if !root_only || !root_dotfiles.is_empty() {
        print_apply_summary(&stats);
    }

    Ok(())
}

fn should_delegate_root_apply(include_root: bool, current_root_dotfiles: usize) -> bool {
    include_root && current_root_dotfiles > 0
}

fn record_managed_target_if_confirmed(
    config: &DotfileConfig,
    db: &Database,
    dotfile: &Dotfile,
    action: &ApplyAction,
) -> Result<()> {
    if !matches!(
        action,
        ApplyAction::Created | ApplyAction::Updated | ApplyAction::AlreadyUpToDate
    ) || !dotfile.target_path.exists()
    {
        return Ok(());
    }

    let source_hash = dotfile.get_file_hash(&dotfile.source_path, true, db)?;
    let target_hash = dotfile.get_file_hash(&dotfile.target_path, false, db)?;
    if source_hash != target_hash {
        return Ok(());
    }

    let Some((repo_name, subdir_name)) = source_identity(config, &dotfile.source_path) else {
        return Ok(());
    };

    db.upsert_managed_target(&ManagedTarget {
        target_path: dotfile.target_path.clone(),
        source_path: dotfile.source_path.clone(),
        repo_name,
        subdir_name,
        applied_hash: target_hash,
        is_root: dotfile.is_root,
    })
}

fn source_identity(config: &DotfileConfig, source_path: &Path) -> Option<(String, String)> {
    // This identifies the source by its unique on-disk repo/subdir prefix;
    // iteration order does not participate in repository priority resolution.
    for repo in &config.repos {
        let repo_path = config.repos_path().join(&repo.name);
        for subdir in config.resolve_active_subdirs(repo) {
            if source_path.starts_with(repo_path.join(&subdir)) {
                return Some((repo.name.clone(), subdir));
            }
        }
    }
    None
}

fn reconcile_removed_targets(
    config: &DotfileConfig,
    db: &Database,
    current_dotfiles: &std::collections::HashMap<PathBuf, Dotfile>,
    is_root: bool,
    reconcile_removed: bool,
) -> Result<()> {
    let current_targets: HashSet<&PathBuf> = current_dotfiles
        .values()
        .filter(|dotfile| dotfile.is_root == is_root)
        .map(|dotfile| &dotfile.target_path)
        .collect();

    for managed in db.get_managed_targets(is_root)? {
        if current_targets.contains(&managed.target_path) {
            continue;
        }
        if !reconcile_removed
            || config.is_path_skipped(&managed.target_path)
            || !provider_is_active(config, &managed)
            || managed.source_path.exists()
            || !source_deletion_is_committed(config, &managed)
        {
            continue;
        }

        if !managed.target_path.exists() {
            db.remove_managed_target(&managed.target_path)?;
            db.remove_hashes_for_path(&managed.source_path)?;
            db.remove_hashes_for_path(&managed.target_path)?;
            continue;
        }

        let current_hash = Dotfile::compute_hash(&managed.target_path).with_context(|| {
            format!(
                "hashing formerly managed target {}",
                managed.target_path.display()
            )
        })?;
        let display = crate::dot::display_path(&managed.target_path, managed.is_root);

        if current_hash == managed.applied_hash {
            std::fs::remove_file(&managed.target_path).with_context(|| {
                format!(
                    "removing target whose source was deleted: {}",
                    managed.target_path.display()
                )
            })?;
            db.remove_hashes_for_path(&managed.source_path)?;
            db.remove_hashes_for_path(&managed.target_path)?;
            db.remove_managed_target(&managed.target_path)?;
            clean_empty_target_parents(&managed.target_path, managed.is_root);
            emit(
                Level::Success,
                "dot.apply.removed",
                &format!(
                    "{} Removed: {} (source deleted from {} / {})",
                    char::from(NerdFont::Check),
                    display.green(),
                    managed.repo_name,
                    managed.subdir_name
                ),
                Some(serde_json::json!({
                    "path": display,
                    "action": "removed",
                    "reason": "source_deleted",
                    "repo": managed.repo_name,
                    "subdir": managed.subdir_name,
                })),
            );
        } else {
            db.remove_hashes_for_path(&managed.source_path)?;
            db.remove_managed_target(&managed.target_path)?;
            emit(
                Level::Warn,
                "dot.apply.preserved_removed_source",
                &format!(
                    "{} Preserved modified file: {} (source deleted from {} / {})",
                    char::from(NerdFont::ShieldAlert),
                    display.yellow(),
                    managed.repo_name,
                    managed.subdir_name
                ),
                Some(serde_json::json!({
                    "path": display,
                    "action": "preserved",
                    "reason": "modified_source_deleted",
                    "repo": managed.repo_name,
                    "subdir": managed.subdir_name,
                })),
            );
        }
    }

    Ok(())
}

fn provider_is_active(config: &DotfileConfig, managed: &ManagedTarget) -> bool {
    let Some(repo) = config
        .repos
        .iter()
        .find(|repo| repo.name == managed.repo_name)
    else {
        return false;
    };
    if !repo.enabled {
        return false;
    }

    if crate::dot::dotfilerepo::DotfileRepo::new(config, managed.repo_name.clone()).is_err() {
        return false;
    }
    let subdir_is_active = config
        .resolve_active_subdirs(repo)
        .contains(&managed.subdir_name);
    if !subdir_is_active {
        return false;
    }

    let active_dir = config
        .repos_path()
        .join(&managed.repo_name)
        .join(&managed.subdir_name);
    managed.source_path.starts_with(&active_dir)
}

fn source_deletion_is_committed(config: &DotfileConfig, managed: &ManagedTarget) -> bool {
    let repo_path = config.repos_path().join(&managed.repo_name);
    crate::common::git::path_deleted_in_head(&repo_path, &managed.source_path).unwrap_or(false)
}

fn clean_empty_target_parents(path: &Path, is_root: bool) {
    if is_root {
        return;
    }

    let home = home_dir();
    let mut dir = path.parent();
    while let Some(parent) = dir {
        if parent == home {
            break;
        }
        if parent.is_dir()
            && std::fs::read_dir(parent).is_ok_and(|mut entries| entries.next().is_none())
        {
            if std::fs::remove_dir(parent).is_err() {
                break;
            }
            dir = parent.parent();
        } else {
            break;
        }
    }
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
    use crate::dot::test_util::EnvGuard;
    use crate::dot::types::RepoMetaData;
    use serial_test::serial;
    use std::fs;
    use tempfile::{TempDir, tempdir};

    struct ApplyTestEnv {
        _dir: TempDir,
        _home_guard: EnvGuard,
        home: PathBuf,
        source: PathBuf,
        config: DotfileConfig,
        db: Database,
    }

    fn setup_apply_test_env() -> ApplyTestEnv {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        let repos_dir = dir.path().join("repos");
        let repo_dir = repos_dir.join("test-repo");
        let dots_dir = repo_dir.join("dots");
        let source = dots_dir.join(".config/app/config.toml");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::create_dir_all(&home).unwrap();
        fs::write(&source, "managed").unwrap();
        fs::write(
            repo_dir.join("instantdots.toml"),
            "name = \"test-repo\"\ndots_dirs = [\"dots\"]\n",
        )
        .unwrap();
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&repo_dir)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "tests@example.com"])
            .current_dir(&repo_dir)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "InstantCLI Tests"])
            .current_dir(&repo_dir)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&repo_dir)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-qm", "Initial commit"])
            .current_dir(&repo_dir)
            .status()
            .unwrap();

        let home_guard = EnvGuard::set("HOME", &home);
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

        ApplyTestEnv {
            _dir: dir,
            _home_guard: home_guard,
            home,
            source,
            config,
            db,
        }
    }

    fn commit_source_deletion(env: &ApplyTestEnv) {
        fs::remove_file(&env.source).unwrap();
        let repo_dir = env.config.repos_path().join("test-repo");
        std::process::Command::new("git")
            .args(["add", "-u"])
            .current_dir(&repo_dir)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-qm", "Delete source"])
            .current_dir(&repo_dir)
            .status()
            .unwrap();
    }

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

    #[test]
    #[serial]
    fn apply_removes_unmodified_target_when_active_source_disappears() {
        let env = setup_apply_test_env();
        let target = env.home.join(".config/app/config.toml");

        apply_all(&env.config, &env.db, false, false).unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "managed");
        assert_eq!(env.db.get_managed_targets(false).unwrap().len(), 1);

        commit_source_deletion(&env);
        apply_all(&env.config, &env.db, false, false).unwrap();

        assert!(!target.exists());
        assert!(env.db.get_managed_targets(false).unwrap().is_empty());
    }

    #[test]
    #[serial]
    fn apply_preserves_modified_target_and_releases_ownership() {
        let env = setup_apply_test_env();
        let target = env.home.join(".config/app/config.toml");

        apply_all(&env.config, &env.db, false, false).unwrap();
        let applied_hash = env
            .db
            .get_managed_targets(false)
            .unwrap()
            .pop()
            .unwrap()
            .applied_hash;
        fs::write(&target, "local change").unwrap();
        crate::dot::dotfile::invalidate_cache(&target);
        commit_source_deletion(&env);
        apply_all(&env.config, &env.db, false, false).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "local change");
        assert!(env.db.get_managed_targets(false).unwrap().is_empty());
        assert!(
            !env.db
                .source_hash_exists(&applied_hash, &env.source)
                .unwrap()
        );

        apply_all(&env.config, &env.db, false, false).unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "local change");
    }

    #[test]
    #[serial]
    fn apply_does_not_reconcile_disabled_provider_or_failed_update_mode() {
        let mut env = setup_apply_test_env();
        let target = env.home.join(".config/app/config.toml");

        apply_all(&env.config, &env.db, false, false).unwrap();
        fs::remove_file(&env.source).unwrap();

        env.config.repos[0].enabled = false;
        apply_all(&env.config, &env.db, false, false).unwrap();
        assert!(target.exists());
        assert_eq!(env.db.get_managed_targets(false).unwrap().len(), 1);

        env.config.repos[0].enabled = true;
        apply_all_with_reconciliation(&env.config, &env.db, false, false, false).unwrap();
        assert!(target.exists());
        assert_eq!(env.db.get_managed_targets(false).unwrap().len(), 1);
    }

    #[test]
    #[serial]
    fn apply_preserves_target_for_uncommitted_source_deletion() {
        let env = setup_apply_test_env();
        let target = env.home.join(".config/app/config.toml");

        apply_all(&env.config, &env.db, false, false).unwrap();
        fs::remove_file(&env.source).unwrap();
        apply_all(&env.config, &env.db, false, false).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "managed");
        assert_eq!(env.db.get_managed_targets(false).unwrap().len(), 1);
    }

    #[test]
    fn stale_root_tracking_alone_does_not_delegate_to_sudo() {
        assert!(!should_delegate_root_apply(true, 0));
        assert!(!should_delegate_root_apply(false, 1));
        assert!(should_delegate_root_apply(true, 1));
    }
}
