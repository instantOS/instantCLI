use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::dotfile::Dotfile;
use crate::dot::git::repo_ops::get_repo_name_for_dotfile;
use crate::dot::utils::{filter_dotfiles_by_path, get_all_dotfiles, resolve_dotfile_path};
use crate::ui::prelude::*;
use anyhow::Context;
use anyhow::Result;
use colored::*;
use std::path::{Path, PathBuf};
use std::process::Stdio;

enum DotfileSkip {
    ReadOnly,
    Unmodified,
}

fn should_skip_dotfile(
    dotfile: &Dotfile,
    config: &Config,
    db: &Database,
    home: &Path,
    verbose: bool,
) -> Result<Option<DotfileSkip>> {
    let relative_path = dotfile
        .target_path
        .strip_prefix(home)
        .unwrap_or(&dotfile.target_path);

    let repo_name = get_repo_name_for_dotfile(dotfile, config);
    let is_read_only = config
        .repos
        .iter()
        .find(|r| r.name == repo_name.as_str())
        .map(|r| r.read_only)
        .unwrap_or(false);

    if is_read_only {
        emit(
            Level::Info,
            "dot.merge.read_only",
            &format!(
                "{} Skipping read-only repository '{}' (~/{})",
                char::from(NerdFont::Lock),
                repo_name,
                relative_path.display()
            ),
            None,
        );
        return Ok(Some(DotfileSkip::ReadOnly));
    }

    if dotfile.is_target_unmodified(db)? {
        if verbose {
            emit(
                Level::Info,
                "dot.merge.unmodified",
                &format!(
                    "{} File ~/{} is already up to date",
                    char::from(NerdFont::Info),
                    relative_path.display()
                ),
                None,
            );
        }
        return Ok(Some(DotfileSkip::Unmodified));
    }

    Ok(None)
}

fn run_nvim_diff(dotfile: &Dotfile, home: &Path) -> Result<()> {
    let relative_path = dotfile
        .target_path
        .strip_prefix(home)
        .unwrap_or(&dotfile.target_path);

    emit(
        Level::Info,
        "dot.merge.start",
        &format!(
            "{} Opening nvim diff for ~/{}",
            char::from(NerdFont::GitMerge),
            relative_path.display().to_string().cyan()
        ),
        None,
    );

    let status = std::process::Command::new("nvim")
        .args([
            "-d",
            &dotfile.target_path.to_string_lossy(),
            &dotfile.source_path.to_string_lossy(),
        ])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("Failed to launch nvim diff")?;

    if !status.success() {
        let code = status.code().unwrap_or(-1);
        if code != 1 {
            emit(
                Level::Warn,
                "dot.merge.nvim_exit",
                &format!(
                    "{} nvim diff exited with code {}",
                    char::from(NerdFont::Warning),
                    code
                ),
                None,
            );
        }
    }

    Ok(())
}

fn report_merge_outcome(
    dotfile: &Dotfile,
    config: &Config,
    db: &Database,
    original_source_hash: &str,
) -> Result<()> {
    let repo_name = get_repo_name_for_dotfile(dotfile, config);
    let new_source_hash = dotfile.get_file_hash(&dotfile.source_path, true, db)?;
    let source_changed = new_source_hash != original_source_hash;
    let files_now_identical = dotfile.is_target_unmodified(db)?;
    let repo_path = config.repos_path().join(repo_name.as_str());

    match (files_now_identical, source_changed) {
        (true, true) => {
            if let Err(e) =
                crate::dot::git::repo_ops::git_add(&repo_path, &dotfile.source_path, false)
            {
                emit(
                    Level::Warn,
                    "dot.merge.stage_failed",
                    &format!(
                        "{} Failed to stage file: {}",
                        char::from(NerdFont::Warning),
                        e
                    ),
                    None,
                );
            } else {
                emit(
                    Level::Success,
                    "dot.merge.resolved_and_staged",
                    &format!(
                        "{} Merge complete and staged for commit\n   Run {} to commit, or {} to commit and push",
                        char::from(NerdFont::Check),
                        "ins dot commit".yellow(),
                        "ins dot push".yellow()
                    ),
                    None,
                );
            }
        }
        (true, false) => {
            emit(
                Level::Success,
                "dot.merge.resolved",
                &format!(
                    "{} Merge complete: files are now identical",
                    char::from(NerdFont::Check)
                ),
                None,
            );
        }
        (false, true) => {
            if let Err(e) =
                crate::dot::git::repo_ops::git_add(&repo_path, &dotfile.source_path, false)
            {
                emit(
                    Level::Warn,
                    "dot.merge.stage_failed",
                    &format!(
                        "{} Failed to stage file: {}",
                        char::from(NerdFont::Warning),
                        e
                    ),
                    None,
                );
            } else {
                emit(
                    Level::Info,
                    "dot.merge.partial",
                    &format!(
                        "{} Partial merge staged (files still differ)\n   Run {} again to continue merging\n   Or use {} to commit current state",
                        char::from(NerdFont::GitBranch),
                        "ins dot merge".cyan(),
                        "ins dot commit".yellow()
                    ),
                    None,
                );
            }
        }
        (false, false) => {
            emit(
                Level::Info,
                "dot.merge.unchanged",
                &format!(
                    "{} No changes made (files still differ)\n   Run {} again to retry",
                    char::from(NerdFont::Info),
                    "ins dot merge".cyan()
                ),
                None,
            );
        }
    }

    Ok(())
}

fn emit_no_modified(target_path: &Path, home: &Path, unmodified_count: usize, verbose: bool) {
    let relative_path = target_path.strip_prefix(home).unwrap_or(target_path);
    let tracked_text = if unmodified_count == 1 {
        "1 tracked file".to_string()
    } else {
        format!("{} tracked files", unmodified_count)
    };

    let message = if verbose {
        format!(
            "{} All tracked dotfiles are already up to date at ~/{} ({})",
            char::from(NerdFont::Info),
            relative_path.display(),
            tracked_text
        )
    } else {
        format!(
            "{} No modified dotfiles found at ~/{} ({}). Use {} to list clean files",
            char::from(NerdFont::Info),
            relative_path.display(),
            tracked_text,
            "--verbose".yellow()
        )
    };

    emit(Level::Info, "dot.merge.no_changes", &message, None);
}

/// Merge a modified dotfile with its source using nvim diff
pub fn merge_dotfile(config: &Config, db: &Database, path: &str, verbose: bool) -> Result<()> {
    let all_dotfiles = get_all_dotfiles(config, db)?;
    let target_path = resolve_dotfile_path(path)?;
    let home = PathBuf::from(shellexpand::tilde("~").to_string());

    let dotfiles_in_path = filter_dotfiles_by_path(&all_dotfiles, &target_path);

    if dotfiles_in_path.is_empty() {
        let relative_path = target_path.strip_prefix(&home).unwrap_or(&target_path);
        emit(
            Level::Warn,
            "dot.merge.not_found",
            &format!(
                "{} No tracked dotfiles found at ~/{}\n   Try {} or {}",
                char::from(NerdFont::Warning),
                relative_path.display(),
                "ins dot status".yellow(),
                "ins dot add".yellow()
            ),
            None,
        );
        return Ok(());
    }

    let mut unmodified_count = 0;
    let mut modified_count = 0;

    for dotfile in dotfiles_in_path {
        match should_skip_dotfile(dotfile, config, db, &home, verbose)? {
            Some(DotfileSkip::ReadOnly) => continue,
            Some(DotfileSkip::Unmodified) => {
                unmodified_count += 1;
                continue;
            }
            None => {}
        }

        modified_count += 1;

        let original_source_hash = dotfile.get_file_hash(&dotfile.source_path, true, db)?;

        run_nvim_diff(dotfile, &home)?;

        report_merge_outcome(dotfile, config, db, &original_source_hash)?;
    }

    if modified_count == 0 {
        emit_no_modified(&target_path, &home, unmodified_count, verbose);
    }

    Ok(())
}
