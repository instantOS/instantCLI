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
                "{} No tracked dotfiles found at ~/{}",
                char::from(NerdFont::Warning),
                relative_path.display()
            ),
            None,
        );
        return Ok(());
    }

    for dotfile in dotfiles_in_path {
        let relative_path = dotfile
            .target_path
            .strip_prefix(&home)
            .unwrap_or(&dotfile.target_path);

        // Check if file is modified
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
            continue;
        }

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

        // Check if files are now the same
        if dotfile.is_target_unmodified(db)? {
            emit(
                Level::Success,
                "dot.merge.resolved",
                &format!(
                    "{} Merge complete: files are now identical",
                    char::from(NerdFont::Check)
                ),
                None,
            );
        } else {
            emit(
                Level::Info,
                "dot.merge.still_different",
                &format!(
                    "{} Files still differ after merge",
                    char::from(NerdFont::Info)
                ),
                None,
            );
        }

        // Check if source file changed (user edited the repo version)
        if new_source_hash != original_source_hash {
            let repo_name = get_repo_name_for_dotfile(&dotfile, config);
            let repo_path = config.repos_path().join(repo_name.as_str());

            emit(
                Level::Info,
                "dot.merge.source_changed",
                &format!(
                    "{} Source file changed in repository\n   Repository path: {}\n   Use {} to commit changes, or {} to push",
                    char::from(NerdFont::GitBranch),
                    repo_path.display().to_string().cyan(),
                    "ins dot commit".yellow(),
                    "ins dot push".yellow()
                ),
                None,
            );
        }
    }

    Ok(())
}
