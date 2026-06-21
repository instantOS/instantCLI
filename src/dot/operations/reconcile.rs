use crate::dot::config::{DotfileConfig, Repo};
use crate::dot::db::{Database, ManagedTarget};
use crate::dot::dotfile::Dotfile;
use crate::dot::utils::{EmptyParentBoundary, clean_empty_parent_dirs};
use crate::ui::prelude::*;
use anyhow::{Context, Result};
use colored::Colorize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

struct SourceOwner<'a> {
    repo: &'a Repo,
    subdir_name: String,
}

pub(super) fn record_managed_target_if_confirmed(
    config: &DotfileConfig,
    db: &Database,
    dotfile: &Dotfile,
    confirmed: bool,
) -> Result<()> {
    if !confirmed || !dotfile.target_path.exists() {
        return Ok(());
    }

    let source_hash = dotfile.get_file_hash(&dotfile.source_path, true, db)?;
    let target_hash = dotfile.get_file_hash(&dotfile.target_path, false, db)?;
    if source_hash != target_hash {
        return Ok(());
    }

    let Some(owner) = source_owner(config, &dotfile.source_path) else {
        return Ok(());
    };

    db.upsert_managed_target(&ManagedTarget {
        target_path: dotfile.target_path.clone(),
        source_path: dotfile.source_path.clone(),
        repo_name: owner.repo.name.clone(),
        subdir_name: owner.subdir_name,
        applied_hash: target_hash,
        is_root: dotfile.is_root,
    })
}

pub(super) fn reconcile_removed_targets(
    config: &DotfileConfig,
    db: &Database,
    current_dotfiles: &HashMap<PathBuf, Dotfile>,
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
            clear_managed_target(db, &managed, true)?;
            continue;
        }

        reconcile_existing_target(db, &managed)?;
    }

    Ok(())
}

fn reconcile_existing_target(db: &Database, managed: &ManagedTarget) -> Result<()> {
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
        clear_managed_target(db, managed, true)?;
        if !managed.is_root {
            clean_empty_parent_dirs(&managed.target_path, EmptyParentBoundary::Home);
        }
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
        clear_managed_target(db, managed, false)?;
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

    Ok(())
}

fn clear_managed_target(
    db: &Database,
    managed: &ManagedTarget,
    remove_target_hashes: bool,
) -> Result<()> {
    db.remove_hashes_for_path(&managed.source_path)?;
    if remove_target_hashes {
        db.remove_hashes_for_path(&managed.target_path)?;
    }
    db.remove_managed_target(&managed.target_path)
}

fn source_owner<'a>(config: &'a DotfileConfig, source_path: &Path) -> Option<SourceOwner<'a>> {
    // Sources have a unique <repos>/<repo>/<subdir> prefix. Iteration order
    // identifies ownership only; repository priority was resolved earlier.
    for repo in &config.repos {
        let repo_path = config.repos_path().join(&repo.name);
        for subdir_name in config.resolve_configured_active_subdirs(repo) {
            if source_path.starts_with(repo_path.join(&subdir_name)) {
                return Some(SourceOwner { repo, subdir_name });
            }
        }
    }
    None
}

fn provider_is_active(config: &DotfileConfig, managed: &ManagedTarget) -> bool {
    let Some(owner) = source_owner(config, &managed.source_path) else {
        return false;
    };

    owner.repo.enabled
        && owner.repo.name == managed.repo_name
        && owner.subdir_name == managed.subdir_name
        && crate::dot::dotfilerepo::DotfileRepo::new(config, managed.repo_name.clone()).is_ok()
}

fn source_deletion_is_committed(config: &DotfileConfig, managed: &ManagedTarget) -> bool {
    let repo_path = config.repos_path().join(&managed.repo_name);
    crate::common::git::path_deleted_in_head(&repo_path, &managed.source_path).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::TildePath;
    use crate::dot::config::Repo;
    use crate::dot::test_util::EnvGuard;
    use crate::dot::types::RepoMetaData;
    use serial_test::serial;
    use std::fs;
    use tempfile::TempDir;

    struct ReconcileTestEnv {
        _dir: TempDir,
        _home_guard: EnvGuard,
        home: PathBuf,
        source: PathBuf,
        config: DotfileConfig,
        db: Database,
    }

    fn setup_reconcile_test_env() -> ReconcileTestEnv {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let repos_dir = dir.path().join("repos");
        let repo_dir = repos_dir.join("test-repo");
        let source = repo_dir.join("dots/.config/app/config.toml");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::create_dir_all(&home).unwrap();
        fs::write(&source, "managed").unwrap();
        fs::write(
            repo_dir.join("instantdots.toml"),
            "name = \"test-repo\"\ndots_dirs = [\"dots\"]\n",
        )
        .unwrap();
        run_git(&repo_dir, &["init", "-q"]);
        run_git(&repo_dir, &["config", "user.email", "tests@example.com"]);
        run_git(&repo_dir, &["config", "user.name", "InstantCLI Tests"]);
        run_git(&repo_dir, &["add", "."]);
        run_git(&repo_dir, &["commit", "-qm", "Initial commit"]);

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

        ReconcileTestEnv {
            _dir: dir,
            _home_guard: home_guard,
            home,
            source,
            config,
            db,
        }
    }

    fn run_git(repo: &Path, args: &[&str]) {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(repo)
                .status()
                .unwrap()
                .success()
        );
    }

    fn commit_source_deletion(env: &ReconcileTestEnv) {
        fs::remove_file(&env.source).unwrap();
        let repo_dir = env.config.repos_path().join("test-repo");
        run_git(&repo_dir, &["add", "-u"]);
        run_git(&repo_dir, &["commit", "-qm", "Delete source"]);
    }

    #[test]
    #[serial]
    fn removes_unmodified_target_when_active_source_disappears() {
        let env = setup_reconcile_test_env();
        let target = env.home.join(".config/app/config.toml");

        crate::dot::operations::apply::apply_all(&env.config, &env.db, false, false).unwrap();
        commit_source_deletion(&env);
        crate::dot::operations::apply::apply_all(&env.config, &env.db, false, false).unwrap();

        assert!(!target.exists());
        assert!(env.db.get_managed_targets(false).unwrap().is_empty());
    }

    #[test]
    #[serial]
    fn preserves_modified_target_and_releases_ownership() {
        let env = setup_reconcile_test_env();
        let target = env.home.join(".config/app/config.toml");

        crate::dot::operations::apply::apply_all(&env.config, &env.db, false, false).unwrap();
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
        crate::dot::operations::apply::apply_all(&env.config, &env.db, false, false).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "local change");
        assert!(env.db.get_managed_targets(false).unwrap().is_empty());
        assert!(
            !env.db
                .source_hash_exists(&applied_hash, &env.source)
                .unwrap()
        );
    }

    #[test]
    #[serial]
    fn does_not_reconcile_disabled_provider_or_failed_update_mode() {
        let mut env = setup_reconcile_test_env();
        let target = env.home.join(".config/app/config.toml");

        crate::dot::operations::apply::apply_all(&env.config, &env.db, false, false).unwrap();
        fs::remove_file(&env.source).unwrap();

        env.config.repos[0].enabled = false;
        crate::dot::operations::apply::apply_all(&env.config, &env.db, false, false).unwrap();
        assert!(target.exists());

        env.config.repos[0].enabled = true;
        let current = crate::dot::get_all_dotfiles(&env.config, &env.db, false).unwrap();
        reconcile_removed_targets(&env.config, &env.db, &current, false, false).unwrap();
        assert!(target.exists());
        assert_eq!(env.db.get_managed_targets(false).unwrap().len(), 1);
    }

    #[test]
    #[serial]
    fn preserves_target_for_uncommitted_source_deletion() {
        let env = setup_reconcile_test_env();
        let target = env.home.join(".config/app/config.toml");

        crate::dot::operations::apply::apply_all(&env.config, &env.db, false, false).unwrap();
        fs::remove_file(&env.source).unwrap();
        crate::dot::operations::apply::apply_all(&env.config, &env.db, false, false).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "managed");
        assert_eq!(env.db.get_managed_targets(false).unwrap().len(), 1);
    }
}
