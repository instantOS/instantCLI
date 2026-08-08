use crate::common::home_dir;
use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use crate::dot::dotfile::Dotfile;
use crate::dot::encryption::classify_encrypted_failure;
use crate::dot::utils::{filter_dotfiles_by_path, get_all_dotfiles, resolve_dotfile_path};
use crate::ui::prelude::*;
use anyhow::Result;
use colored::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Reset modified dotfiles to their original state
pub fn reset_modified(
    config: &DotfileConfig,
    db: &Database,
    path: &str,
    include_root: bool,
    root_only: bool,
) -> Result<()> {
    let all_dotfiles = get_all_dotfiles(config, db, include_root || root_only)?;
    // Resolve without requiring existence: resetting a missing target
    // re-creates it from its source (see below).
    let target_path = resolve_dotfile_path(path, include_root, false)?;
    let home = home_dir();

    // Filter to dotfiles within the specified path
    let mut dotfiles_in_path = filter_dotfiles_by_path(&all_dotfiles, &target_path);

    // The requested path may not exist on disk (target deleted or never
    // applied). Match it against tracked files so that
    // `ins dot reset <name>` restores a missing target from its source.
    if dotfiles_in_path.is_empty()
        && !target_path.exists()
        && let Some(matches) = match_missing_target(&all_dotfiles, &target_path, path)?
    {
        dotfiles_in_path = matches;
    }

    if dotfiles_in_path.is_empty() {
        let relative_path = target_path.strip_prefix(&home).unwrap_or(&target_path);
        emit(
            Level::Info,
            "dot.reset.no_files",
            &format!(
                "{} No tracked dotfiles found in ~/{} ",
                char::from(NerdFont::Info),
                relative_path.display()
            ),
            None,
        );
        return Ok(());
    }

    let mut reset_count = 0;
    let mut clean_count = 0;

    for dotfile in dotfiles_in_path {
        if root_only && !dotfile.is_root {
            continue;
        }
        if !root_only && dotfile.is_root {
            continue;
        }

        // A missing target cannot be "unmodified": reset re-creates it
        // from its source, so deleted tracked files are restored instead
        // of being skipped as clean.
        let is_unmodified = if !dotfile.target_path.exists() {
            false
        } else {
            match dotfile.is_target_unmodified(db) {
                Ok(unmodified) => unmodified,
                Err(err) if dotfile.kind == crate::dot::dotfile::SourceKind::Age => {
                    let reason = classify_encrypted_failure(&err);
                    emit(
                        Level::Warn,
                        "dot.reset.skipped_encrypted",
                        &format!(
                            "{} Skipped reset of encrypted file ({}): {}",
                            char::from(NerdFont::ShieldAlert),
                            reason.label(),
                            crate::dot::display_path(&dotfile.target_path, dotfile.is_root)
                                .yellow()
                        ),
                        Some(serde_json::json!({
                            "path": crate::dot::display_path(&dotfile.target_path, dotfile.is_root),
                            "reason": reason.code()
                        })),
                    );
                    continue;
                }
                Err(err) => return Err(err),
            }
        };

        if !is_unmodified {
            match dotfile.reset(db) {
                Ok(_) => {
                    let relative_path =
                        crate::dot::display_path(&dotfile.target_path, dotfile.is_root);
                    println!(
                        "{} Reset {} ",
                        char::from(NerdFont::Check),
                        relative_path.green()
                    );
                    reset_count += 1;
                }
                Err(err) if dotfile.kind == crate::dot::dotfile::SourceKind::Age => {
                    let reason = classify_encrypted_failure(&err);
                    emit(
                        Level::Warn,
                        "dot.reset.skipped_encrypted",
                        &format!(
                            "{} Skipped reset of encrypted file ({}): {}",
                            char::from(NerdFont::ShieldAlert),
                            reason.label(),
                            crate::dot::display_path(&dotfile.target_path, dotfile.is_root)
                                .yellow()
                        ),
                        Some(serde_json::json!({
                            "path": crate::dot::display_path(&dotfile.target_path, dotfile.is_root),
                            "reason": reason.code()
                        })),
                    );
                }
                Err(err) => return Err(err),
            }
        } else {
            clean_count += 1;
        }
    }

    if !root_only && include_root {
        let root_files: Vec<_> = filter_dotfiles_by_path(&all_dotfiles, &target_path)
            .into_iter()
            .filter(|d| d.is_root)
            .collect();

        if !root_files.is_empty() {
            let home_dir = home_dir();
            let home_dir_str = home_dir.to_string_lossy();
            emit(
                Level::Info,
                "dot.reset.root_files",
                &format!(
                    "{} Resetting {} root dotfile(s) (requires sudo)",
                    char::from(NerdFont::ShieldCheck),
                    root_files.len()
                ),
                None,
            );

            let status = std::process::Command::new("sudo")
                .arg("ins")
                .arg("dot")
                .arg("reset")
                .arg(path)
                .arg("--root-only")
                .arg("--home")
                .arg(home_dir_str.as_ref())
                .status();

            if let Err(e) = status {
                emit(
                    Level::Warn,
                    "dot.reset.root_failed",
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
                    "dot.reset.root_failed",
                    &format!(
                        "{} Resetting root dotfiles failed or was cancelled",
                        char::from(NerdFont::Warning)
                    ),
                    None,
                );
            }
        }
    }

    db.cleanup_hashes(config.hash_cleanup_days)?;

    // Summary
    if reset_count > 0 {
        let reset_text = if reset_count == 1 {
            "1 file".to_string()
        } else {
            format!("{} files", reset_count)
        };

        let msg = if clean_count > 0 {
            let clean_text = if clean_count == 1 {
                "1 already clean".to_string()
            } else {
                format!("{} already clean", clean_count)
            };
            format!(
                "{} Reset {}, {}",
                char::from(NerdFont::Check),
                reset_text,
                clean_text
            )
        } else {
            format!("{} Reset {}", char::from(NerdFont::Check), reset_text)
        };

        emit(Level::Success, "dot.reset.complete", &msg, None);
    } else {
        let clean_text = if clean_count == 1 {
            "1 file is already clean".to_string()
        } else {
            format!("All {} files already clean", clean_count)
        };

        emit(
            Level::Info,
            "dot.reset.no_changes",
            &format!(
                "{} {} - no reset needed",
                char::from(NerdFont::Info),
                clean_text
            ),
            None,
        );
    }

    Ok(())
}

/// Match a requested path that does not exist on disk against tracked
/// dotfiles. Tries the exact resolved target path first, then a unique
/// basename match (the request may carry an `.age` suffix for encrypted
/// sources, whose tracked targets use plaintext names).
///
/// Returns `Ok(None)` when nothing matches (the caller keeps its regular
/// "no tracked files" message) and an error when the basename is ambiguous.
fn match_missing_target<'a>(
    all_dotfiles: &'a HashMap<PathBuf, Dotfile>,
    target_path: &Path,
    requested: &str,
) -> Result<Option<Vec<&'a Dotfile>>> {
    if let Some(dotfile) = all_dotfiles.get(target_path) {
        return Ok(Some(vec![dotfile]));
    }

    let Some(requested_name) = Path::new(requested).file_name().and_then(|n| n.to_str()) else {
        return Ok(None);
    };

    let matches: Vec<_> = all_dotfiles
        .values()
        .filter(|dotfile| {
            let target_name = dotfile
                .target_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            target_name == requested_name
                || requested_name.strip_suffix(".age") == Some(target_name)
        })
        .collect();

    match matches.as_slice() {
        [] => Ok(None),
        [single] => Ok(Some(vec![*single])),
        multiple => Err(anyhow::anyhow!(
            "Path '{}' does not exist and its name matches {} tracked files:\n  {}\n\
             Use the full target path, e.g. 'ins dot reset ~/...', to pick one.",
            requested,
            multiple.len(),
            multiple
                .iter()
                .map(|dotfile| crate::dot::display_path(&dotfile.target_path, dotfile.is_root))
                .collect::<Vec<_>>()
                .join("\n  ")
        )),
    }
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

    struct ResetTestEnv {
        _dir: TempDir,
        _home_guard: EnvGuard,
        home: PathBuf,
        config: DotfileConfig,
        db: Database,
    }

    fn setup_reset_test_env() -> ResetTestEnv {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let repos_dir = dir.path().join("repos");
        let repo_dir = repos_dir.join("test-repo");
        let dots_dir = repo_dir.join("dots");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&repos_dir).unwrap();
        fs::create_dir_all(&dots_dir).unwrap();

        // One unique-basename source plus two sources sharing the basename
        // `shared.json` to exercise ambiguous-name handling.
        let sources = [
            (".prime/agent/reset_unique_test.json", "unique source\n"),
            (".prime/agent/shared.json", "prime shared\n"),
            (".config/app/shared.json", "config shared\n"),
        ];
        for (relative, content) in sources {
            let source = dots_dir.join(relative);
            fs::create_dir_all(source.parent().unwrap()).unwrap();
            fs::write(&source, content).unwrap();
        }

        fs::write(
            repo_dir.join("instantdots.toml"),
            "name = \"test-repo\"\ndots_dirs = [\"dots\"]\n",
        )
        .unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(&repo_dir)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            std::process::Command::new("git")
                .args(["config", "user.email", "tests@example.com"])
                .current_dir(&repo_dir)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            std::process::Command::new("git")
                .args(["config", "user.name", "InstantCLI Tests"])
                .current_dir(&repo_dir)
                .status()
                .unwrap()
                .success()
        );

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

        ResetTestEnv {
            _dir: dir,
            _home_guard: home_guard,
            home,
            config,
            db,
        }
    }

    #[test]
    #[serial]
    fn reset_recreates_missing_target_by_full_path() {
        let env = setup_reset_test_env();
        let target = env.home.join(".prime/agent/reset_unique_test.json");
        assert!(!target.exists());

        reset_modified(&env.config, &env.db, target.to_str().unwrap(), false, false).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "unique source\n");
    }

    #[test]
    #[serial]
    fn reset_recreates_missing_target_by_basename() {
        let env = setup_reset_test_env();
        let target = env.home.join(".prime/agent/reset_unique_test.json");
        assert!(!target.exists());

        reset_modified(&env.config, &env.db, "reset_unique_test.json", false, false).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "unique source\n");
    }

    #[test]
    #[serial]
    fn reset_rejects_ambiguous_basename_for_missing_target() {
        let env = setup_reset_test_env();
        let err = reset_modified(&env.config, &env.db, "shared.json", false, false).unwrap_err();
        assert!(err.to_string().contains("matches 2 tracked files"));
        assert!(err.to_string().contains("~/.prime/agent/shared.json"));
        assert!(err.to_string().contains("~/.config/app/shared.json"));
    }

    #[test]
    #[serial]
    fn reset_still_restores_modified_existing_target() {
        let env = setup_reset_test_env();
        let target = env.home.join(".prime/agent/reset_unique_test.json");

        reset_modified(&env.config, &env.db, target.to_str().unwrap(), false, false).unwrap();
        fs::write(&target, "user change").unwrap();

        reset_modified(&env.config, &env.db, target.to_str().unwrap(), false, false).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "unique source\n");
    }
}
