use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use colored::*;

use crate::common::home_dir;
use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use crate::dot::dotfile::{Dotfile, SourceKind};
use crate::dot::encryption::classify_encrypted_failure;
use crate::dot::utils::{get_all_dotfiles, resolve_dotfile_path};
use crate::ui::prelude::*;

/// Restore selected dotfiles to their current repository sources.
pub fn reset_modified(
    config: &DotfileConfig,
    db: &Database,
    path: &str,
    include_root: bool,
    root_only: bool,
) -> Result<()> {
    let all_dotfiles = get_all_dotfiles(config, db, include_root || root_only)?;
    // Missing targets are valid: reset recreates them from their sources.
    let target_path = resolve_dotfile_path(path, include_root || root_only, false)?;
    let selected = select_reset_files(&all_dotfiles, &target_path, path, root_only)?;
    if selected.is_empty() {
        emit(
            Level::Info,
            "dot.reset.no_files",
            &format!(
                "{} No tracked dotfiles found in {}",
                char::from(NerdFont::Info),
                crate::dot::display_path(&target_path, !target_path.starts_with(home_dir()))
            ),
            None,
        );
        return Ok(());
    }

    let mut summary = ResetSummary::default();
    for dotfile in selected.iter().filter(|file| file.is_root == root_only) {
        summary.record(reset_file(dotfile, db)?);
    }

    let root_count = selected.iter().filter(|file| file.is_root).count();
    if !root_only && include_root && root_count > 0 {
        // A basename fallback selects one file. Forward its exact target so
        // the child cannot lose or reinterpret that selection.
        let root_path = if selected.len() == 1 {
            &selected[0].target_path
        } else {
            &target_path
        };
        delegate_root_reset(root_path, root_count)?;
    }

    db.cleanup_hashes(config.hash_cleanup_days)?;
    summary.emit();
    Ok(())
}

fn select_reset_files<'a>(
    all_dotfiles: &'a HashMap<PathBuf, Dotfile>,
    target_path: &Path,
    requested: &str,
    root_only: bool,
) -> Result<Vec<&'a Dotfile>> {
    // Exclude home files before basename matching in the privileged child.
    let eligible: Vec<_> = all_dotfiles
        .values()
        .filter(|file| !root_only || file.is_root)
        .collect();
    let selected: Vec<_> = eligible
        .iter()
        .copied()
        .filter(|file| file.target_path.starts_with(target_path))
        .collect();
    if selected.is_empty() && !target_path.exists() {
        return Ok(match_missing_target(&eligible, target_path, requested)?.unwrap_or_default());
    }
    Ok(selected)
}

#[derive(Debug, PartialEq, Eq)]
enum ResetOutcome {
    Reset,
    Clean,
    Skipped,
}

fn reset_file(dotfile: &Dotfile, db: &Database) -> Result<ResetOutcome> {
    // Compare against the current source, including targets that still match
    // an older known source version. Handle failures from both stages alike.
    let result = dotfile.is_outdated(db).and_then(|outdated| {
        if outdated {
            dotfile.reset(db)?;
            Ok(ResetOutcome::Reset)
        } else {
            Ok(ResetOutcome::Clean)
        }
    });
    let path = crate::dot::display_path(&dotfile.target_path, dotfile.is_root);
    match result {
        Ok(ResetOutcome::Reset) => {
            emit(
                Level::Success,
                "dot.reset.file",
                &format!("{} Reset {}", char::from(NerdFont::Check), path.green()),
                None,
            );
            Ok(ResetOutcome::Reset)
        }
        Err(error) if dotfile.kind == SourceKind::Age => {
            let reason = classify_encrypted_failure(&error);
            emit(
                Level::Warn,
                "dot.reset.skipped_encrypted",
                &format!(
                    "{} Skipped reset of encrypted file ({}): {}",
                    char::from(NerdFont::ShieldAlert),
                    reason.label(),
                    path.yellow()
                ),
                Some(serde_json::json!({ "path": path, "reason": reason.code() })),
            );
            Ok(ResetOutcome::Skipped)
        }
        other => other.with_context(|| format!("Failed to reset {path}")),
    }
}

fn delegate_root_reset(path: &Path, count: usize) -> Result<()> {
    emit(
        Level::Info,
        "dot.reset.root_files",
        &format!(
            "{} Resetting {} root dotfile(s) (requires sudo)",
            char::from(NerdFont::ShieldCheck),
            count
        ),
        None,
    );
    let mut command = Command::new("sudo");
    command
        .args(["ins", "dot", "reset"])
        .arg(path)
        .arg("--root-only")
        .arg("--home")
        .arg(home_dir());
    run_root_reset(&mut command).inspect_err(|error| {
        emit(
            Level::Warn,
            "dot.reset.root_failed",
            &format!("{} {error:#}", char::from(NerdFont::Warning)),
            None,
        );
    })
}

fn run_root_reset(command: &mut Command) -> Result<()> {
    let status = command
        .status()
        .context("Failed to spawn sudo for root dotfiles")?;
    if !status.success() {
        bail!("Resetting root dotfiles failed or was cancelled ({status})");
    }
    Ok(())
}

#[derive(Default)]
struct ResetSummary {
    reset: usize,
    clean: usize,
    skipped: usize,
}

impl ResetSummary {
    fn record(&mut self, outcome: ResetOutcome) {
        match outcome {
            ResetOutcome::Reset => self.reset += 1,
            ResetOutcome::Clean => self.clean += 1,
            ResetOutcome::Skipped => self.skipped += 1,
        }
    }

    fn message(&self) -> String {
        format!(
            "Reset {} {}, {} already clean, {} skipped",
            self.reset,
            if self.reset == 1 { "file" } else { "files" },
            self.clean,
            self.skipped
        )
    }

    fn emit(&self) {
        // The sudo child reports its own results; a parent processing only
        // root files has no local outcome to report.
        if self.reset + self.clean + self.skipped == 0 {
            return;
        }
        let (level, event) = if self.skipped > 0 {
            (Level::Warn, "dot.reset.incomplete")
        } else if self.reset > 0 {
            (Level::Success, "dot.reset.complete")
        } else {
            (Level::Info, "dot.reset.no_changes")
        };
        emit(
            level,
            event,
            &self.message(),
            Some(serde_json::json!({
                "reset": self.reset, "clean": self.clean, "skipped": self.skipped,
            })),
        );
    }
}

/// Match a requested path that does not exist on disk against tracked
/// dotfiles. Tries the exact resolved target path first, then a unique
/// basename match (the request may carry an `.age` suffix for encrypted
/// sources, whose tracked targets use plaintext names).
///
/// Returns `Ok(None)` when nothing matches (the caller keeps its regular
/// "no tracked files" message) and an error when the basename is ambiguous.
fn match_missing_target<'a>(
    dotfiles: &[&'a Dotfile],
    target_path: &Path,
    requested: &str,
) -> Result<Option<Vec<&'a Dotfile>>> {
    if let Some(dotfile) = dotfiles
        .iter()
        .copied()
        .find(|file| file.target_path == target_path)
    {
        return Ok(Some(vec![dotfile]));
    }

    let Some(requested_name) = Path::new(requested).file_name().and_then(|n| n.to_str()) else {
        return Ok(None);
    };

    let matches: Vec<_> = dotfiles
        .iter()
        .copied()
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
    fn reset_updates_clean_outdated_target_without_applying_unrelated_files() -> Result<()> {
        let env = setup_reset_test_env();
        let target = env.home.join(".prime/agent/reset_unique_test.json");
        let dotfiles = get_all_dotfiles(&env.config, &env.db, false)?;
        let dotfile = dotfiles
            .values()
            .find(|dotfile| dotfile.target_path == target)
            .ok_or_else(|| anyhow::anyhow!("test dotfile not found"))?;

        dotfile.reset(&env.db)?;
        fs::write(&dotfile.source_path, "updated source\n")?;
        assert!(dotfile.is_target_unmodified(&env.db)?);
        assert!(dotfile.is_outdated(&env.db)?);

        reset_modified(
            &env.config,
            &env.db,
            &target.to_string_lossy(),
            false,
            false,
        )?;

        assert_eq!(fs::read_to_string(&target)?, "updated source\n");
        assert!(!dotfile.is_outdated(&env.db)?);
        assert!(dotfile.is_target_unmodified(&env.db)?);
        assert!(!env.home.join(".prime/agent/shared.json").exists());
        assert!(!env.home.join(".config/app/shared.json").exists());
        Ok(())
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

    fn add_root_source(env: &mut ResetTestEnv) -> Result<PathBuf> {
        // Root targets live outside the test home, but stay in its temp dir.
        let target = env._dir.path().join("root-target/reset_root_test.conf");
        let source_dir = env.config.repos_path().join("test-repo/dots_root");
        let source = source_dir.join(target.strip_prefix("/")?);
        fs::create_dir_all(source.parent().context("source parent")?)?;
        fs::write(source, "root source\n")?;
        env.config.repos[0].active_subdirectories = Some(vec!["dots".into(), "dots_root".into()]);
        env.config.repos[0]
            .metadata
            .as_mut()
            .context("metadata")?
            .dots_dirs
            .push("dots_root".into());
        Ok(target)
    }

    #[test]
    #[serial]
    fn root_only_restores_absolute_target_outside_home() -> Result<()> {
        let mut env = setup_reset_test_env();
        let target = add_root_source(&mut env)?;
        reset_modified(
            &env.config,
            &env.db,
            target.to_str().context("target path")?,
            false,
            true,
        )?;
        assert_eq!(fs::read_to_string(target)?, "root source\n");
        assert!(
            !env.home
                .join(".prime/agent/reset_unique_test.json")
                .exists()
        );
        Ok(())
    }

    #[test]
    #[serial]
    fn root_only_basename_matching_excludes_home_files() -> Result<()> {
        let mut env = setup_reset_test_env();
        let target = add_root_source(&mut env)?;
        let home_source = env
            .config
            .repos_path()
            .join("test-repo/dots/reset_root_test.conf");
        fs::write(home_source, "home source\n")?;
        reset_modified(&env.config, &env.db, "reset_root_test.conf", false, true)?;
        assert_eq!(fs::read_to_string(target)?, "root source\n");
        assert!(!env.home.join("reset_root_test.conf").exists());
        Ok(())
    }

    #[test]
    #[serial]
    fn basename_root_selection_is_delegated_as_exact_target() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let mut env = setup_reset_test_env();
        let target = add_root_source(&mut env)?;
        let bin_dir = env._dir.path().join("bin");
        fs::create_dir_all(&bin_dir)?;
        let sudo = bin_dir.join("sudo");
        // Capture arguments without ever invoking privilege escalation.
        fs::write(
            &sudo,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$RESET_TEST_ARGS\"\nexit 0\n",
        )?;
        fs::set_permissions(&sudo, fs::Permissions::from_mode(0o755))?;
        let args_path = env._dir.path().join("sudo-args");
        let _path_guard = EnvGuard::set("PATH", &bin_dir);
        let _args_guard = EnvGuard::set("RESET_TEST_ARGS", &args_path);
        reset_modified(&env.config, &env.db, "reset_root_test.conf", true, false)?;
        let args = fs::read_to_string(&args_path)?;
        let args: Vec<_> = args.lines().collect();
        assert_eq!(
            args,
            vec![
                "ins",
                "dot",
                "reset",
                target.to_str().context("target path")?,
                "--root-only",
                "--home",
                env.home.to_str().context("home path")?
            ]
        );
        assert!(!target.exists());

        fs::write(&sudo, "#!/bin/sh\nexit 7\n")?;
        let error = reset_modified(&env.config, &env.db, "reset_root_test.conf", true, false)
            .expect_err("sudo failure must propagate");
        assert!(error.to_string().contains("failed or was cancelled"));
        Ok(())
    }

    #[test]
    fn root_command_spawn_failure_is_an_error() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut command = Command::new(dir.path().join("missing-sudo"));
        let error = run_root_reset(&mut command).expect_err("spawn failure must propagate");
        assert!(error.to_string().contains("Failed to spawn sudo"));
        Ok(())
    }

    #[test]
    #[serial]
    fn encrypted_failures_at_either_stage_are_counted_as_skipped() -> Result<()> {
        let env = setup_reset_test_env();
        let source = env._dir.path().join("invalid.age");
        fs::write(&source, "invalid age ciphertext")?;
        let target = env.home.join("encrypted-target");
        let dotfile = Dotfile::new(source, target.clone(), false);
        let mut summary = ResetSummary::default();

        // A missing target fails during reset; an existing target fails while
        // comparing hashes. Neither case may be reported as clean or reset.
        for existing in [false, true] {
            if existing {
                fs::write(&target, "keep me")?;
            }
            let outcome = reset_file(&dotfile, &env.db)?;
            assert_eq!(outcome, ResetOutcome::Skipped);
            summary.record(outcome);
            if existing {
                assert_eq!(fs::read_to_string(&target)?, "keep me");
            } else {
                assert!(!target.exists());
            }
        }
        assert_eq!(
            summary.message(),
            "Reset 0 files, 0 already clean, 2 skipped"
        );
        Ok(())
    }

    #[test]
    #[serial]
    fn reset_file_distinguishes_clean_files_and_plain_errors() -> Result<()> {
        let env = setup_reset_test_env();
        let source = env._dir.path().join("source");
        let target = env.home.join("target");
        fs::write(&source, "source contents")?;
        let dotfile = Dotfile::new(source.clone(), target.clone(), false);
        assert_eq!(reset_file(&dotfile, &env.db)?, ResetOutcome::Reset);
        assert_eq!(reset_file(&dotfile, &env.db)?, ResetOutcome::Clean);
        fs::remove_file(source)?;
        let error = reset_file(&dotfile, &env.db).expect_err("plain failure must propagate");
        assert!(error.to_string().contains("Failed to reset"));
        assert_eq!(fs::read_to_string(target)?, "source contents");
        Ok(())
    }
}
