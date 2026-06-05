use crate::common::home_dir;
use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use crate::dot::dotfile::Dotfile;
use crate::dot::dotfilerepo::DotfileRepo;
use crate::dot::git::repo_ops::get_repo_name_for_dotfile;
use crate::dot::utils::{filter_dotfiles_by_path, get_all_dotfiles, resolve_dotfile_path};
use crate::ui::prelude::*;
use anyhow::Context;
use anyhow::Result;
use colored::*;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;

enum DotfileSkip {
    ReadOnly,
    Unmodified,
}

fn should_skip_dotfile(
    dotfile: &Dotfile,
    config: &DotfileConfig,
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
    config: &DotfileConfig,
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

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd)]
struct InactiveDotfileMatch {
    repo_name: String,
    subdir_name: String,
}

fn relative_target_path(target_path: &Path, home: &Path) -> PathBuf {
    target_path
        .strip_prefix(home)
        .unwrap_or_else(|_| {
            target_path
                .strip_prefix(Path::new("/"))
                .unwrap_or(target_path)
        })
        .to_path_buf()
}

fn age_source_path(source_path: &Path) -> PathBuf {
    let mut age_path = source_path.as_os_str().to_os_string();
    age_path.push(".age");
    PathBuf::from(age_path)
}

fn inactive_dir_contains_target(dir_path: &Path, relative_target: &Path) -> bool {
    let source_path = dir_path.join(relative_target);
    source_path.exists() || age_source_path(&source_path).exists()
}

fn find_inactive_dotfile_matches(
    config: &DotfileConfig,
    target_path: &Path,
) -> Result<Vec<InactiveDotfileMatch>> {
    let home = home_dir();
    let relative_target = relative_target_path(target_path, &home);
    let mut matches = Vec::new();

    for repo in &config.repos {
        if !repo.enabled {
            continue;
        }

        let dotfile_repo = match DotfileRepo::new(config, repo.name.clone()) {
            Ok(repo) => repo,
            Err(e) => {
                eprintln!(
                    "{}",
                    format!("Warning: skipping repo '{}': {}", repo.name, e).yellow()
                );
                continue;
            }
        };

        for dir in dotfile_repo
            .dotfile_dirs
            .iter()
            .filter(|dir| !dir.is_active)
        {
            if !inactive_dir_contains_target(&dir.path, &relative_target) {
                continue;
            }

            let subdir_name = dir
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("unknown")
                .to_string();
            matches.push(InactiveDotfileMatch {
                repo_name: dotfile_repo.name.clone(),
                subdir_name,
            });
        }
    }

    matches.sort();
    matches.dedup();
    Ok(matches)
}

fn emit_not_found_with_inactive_hint(
    config: &DotfileConfig,
    target_path: &Path,
    home: &Path,
) -> Result<()> {
    let relative_path = target_path.strip_prefix(home).unwrap_or(target_path);
    let inactive_matches = find_inactive_dotfile_matches(config, target_path)?;

    let mut message = format!(
        "{} No tracked dotfiles found at ~/{}\n   Try {} or {}",
        char::from(NerdFont::Warning),
        relative_path.display(),
        "ins dot status".yellow(),
        "ins dot add".yellow()
    );

    match inactive_matches.as_slice() {
        [] => {}
        [m] => {
            message.push_str(&format!(
                "\n   Found a matching source in inactive subdir '{}:{}'\n   Enable it with {}",
                m.repo_name.cyan(),
                m.subdir_name.cyan(),
                format!(
                    "ins dot repo subdirs enable {} {}",
                    m.repo_name, m.subdir_name
                )
                .yellow()
            ));
        }
        matches => {
            let locations = matches
                .iter()
                .map(|m| format!("{}:{}", m.repo_name, m.subdir_name))
                .collect::<Vec<_>>()
                .join(", ");
            message.push_str(&format!(
                "\n   Found matching sources in inactive subdirs: {}\n   Enable one with {}",
                locations.cyan(),
                "ins dot repo subdirs enable <repo> <subdir>".yellow()
            ));
        }
    }

    emit(Level::Warn, "dot.merge.not_found", &message, None);
    Ok(())
}

/// Merge a modified dotfile with its source using nvim diff
pub fn merge_dotfile(
    config: &DotfileConfig,
    db: &Database,
    path: &str,
    verbose: bool,
) -> Result<()> {
    let all_dotfiles = get_all_dotfiles(config, db, false)?;
    let target_path = resolve_dotfile_path(path, false, true)?;
    let home = home_dir();

    let dotfiles_in_path = filter_dotfiles_by_path(&all_dotfiles, &target_path);

    if dotfiles_in_path.is_empty() {
        emit_not_found_with_inactive_hint(config, &target_path, &home)?;
        return Ok(());
    }

    let mut unmodified_count = 0;
    let mut modified_count = 0;

    for dotfile in dotfiles_in_path {
        if dotfile.kind == crate::dot::dotfile::SourceKind::Age {
            emit(
                Level::Warn,
                "dot.merge.encrypted_unsupported",
                &format!(
                    "{} Merging is not yet supported for encrypted files: {}",
                    char::from(NerdFont::ShieldAlert),
                    crate::dot::display_path(&dotfile.target_path, dotfile.is_root).yellow()
                ),
                None,
            );
            continue;
        }

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
