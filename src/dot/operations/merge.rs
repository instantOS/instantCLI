use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::git::repo_ops::get_repo_name_for_dotfile;
use crate::dot::utils::{filter_dotfiles_by_path, get_all_dotfiles, resolve_dotfile_path};
use crate::ui::prelude::*;
use anyhow::Context;
use anyhow::Result;
use colored::*;
use std::process::Stdio;

/// Merge a modified dotfile with its source using nvim diff
pub fn merge_dotfile(config: &Config, db: &Database, path: &str, verbose: bool) -> Result<()> {
    let all_dotfiles = get_all_dotfiles(config, db)?;
    let target_path = resolve_dotfile_path(path)?;
    let home = std::path::PathBuf::from(shellexpand::tilde("~").to_string());

    // Filter to dotfiles within the specified path
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
        let relative_path = dotfile
            .target_path
            .strip_prefix(&home)
            .unwrap_or(&dotfile.target_path);

        // Check if the owning repo is read-only
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
            continue;
        }

        // Check if file is modified
        if dotfile.is_target_unmodified(db)? {
            unmodified_count += 1;
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
            continue;
        }

        modified_count += 1;

        // Store original source hash for comparison after merge
        let original_source_hash = dotfile.get_file_hash(&dotfile.source_path, true, db)?;

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

        // Run nvim diff with interactive terminal control
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
            // Exit code 1 is normal for nvim when no changes were made
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

        // Compute new source hash after merge
        let new_source_hash = dotfile.get_file_hash(&dotfile.source_path, true, db)?;
        let source_changed = new_source_hash != original_source_hash;
        let files_now_identical = dotfile.is_target_unmodified(db)?;

        let repo_path = config.repos_path().join(repo_name.as_str());

        match (files_now_identical, source_changed) {
            (true, true) => {
                // Best case: user merged changes into source, files now match
                // Auto-stage the file
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
                // Files are identical but source wasn't changed
                // This means user chose to keep the source version
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
                // User edited source but files still differ
                // Auto-stage so their work isn't lost
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
                // No changes made to source, files still differ
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
    }

    if modified_count == 0 {
        let relative_path = target_path.strip_prefix(&home).unwrap_or(&target_path);
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

    Ok(())
}
