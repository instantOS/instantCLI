use crate::dot::config;
use crate::dot::git::{get_dotfile_dir_name, get_repo_name_for_dotfile};
use crate::ui::prelude::*;
use anyhow::{Context, Result};
use colored::*;
use serde_json;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, serde::Serialize)]
pub enum DotFileStatus {
    Modified,
    Outdated,
    Clean,
}

impl std::fmt::Display for DotFileStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DotFileStatus::Modified => write!(
                f,
                "{} {}",
                crate::ui::nerd_font::NerdFont::Edit.to_string().yellow(),
                "modified".yellow()
            ),
            DotFileStatus::Outdated => write!(
                f,
                "{} {}",
                crate::ui::nerd_font::NerdFont::Sync2.to_string().blue(),
                "outdated".blue()
            ),
            DotFileStatus::Clean => write!(
                f,
                "{} {}",
                crate::ui::nerd_font::NerdFont::CheckCircle
                    .to_string()
                    .green(),
                "clean".green()
            ),
        }
    }
}

/// File information with metadata for display
pub struct FileInfo {
    pub target_path: PathBuf,
    pub dotfile: crate::dot::Dotfile,
    pub repo_name: crate::dot::RepoName,
    pub dotfile_dir: String,
    pub is_overridden: bool,
}

/// Status summary statistics
pub struct StatusSummary {
    pub total_files: usize,
    pub clean_count: usize,
    pub modified_count: usize,
    pub outdated_count: usize,
}

/// Show status for a single file
pub fn show_single_file_status(
    path_str: &str,
    all_dotfiles: &HashMap<PathBuf, crate::dot::Dotfile>,
    cfg: &config::Config,
    db: &crate::dot::db::Database,
) -> Result<()> {
    let target_path = crate::dot::resolve_dotfile_path(path_str)?;

    if target_path.is_dir() {
        let mut matching: Vec<_> = all_dotfiles
            .iter()
            .filter(|(path, _)| path.starts_with(&target_path))
            .collect();

        if matching.is_empty() {
            match get_output_format() {
                OutputFormat::Json => {
                    let status_data = serde_json::json!({
                        "path": target_path.display().to_string(),
                        "tracked": false,
                        "type": "directory"
                    });
                    emit(
                        Level::Info,
                        "dot.status.directory",
                        "Directory not tracked",
                        Some(status_data),
                    );
                }
                OutputFormat::Text => {
                    println!("{} -> not tracked", target_path.display());
                }
            }
            return Ok(());
        }

        matching.sort_by(|(a, _), (b, _)| a.cmp(b));

        match get_output_format() {
            OutputFormat::Json => {
                let home = dirs::home_dir().unwrap_or_default();
                let files: Vec<_> = matching
                    .into_iter()
                    .map(|(path, dotfile)| {
                        let status = get_dotfile_status(dotfile, db);
                        let repo_name = get_repo_name_for_dotfile(dotfile, cfg);
                        let dotfile_dir = get_dotfile_dir_name(dotfile, cfg);
                        let relative_path = path.strip_prefix(&home).unwrap_or(path);
                        serde_json::json!({
                            "path": format!("~/{}", relative_path.display()),
                            "status": status,
                            "source": dotfile.source_path.display().to_string(),
                            "repo": repo_name.as_str(),
                            "dotfile_dir": dotfile_dir
                        })
                    })
                    .collect();

                let status_data = serde_json::json!({
                    "path": target_path.display().to_string(),
                    "tracked": true,
                    "type": "directory",
                    "files": files
                });

                emit(
                    Level::Info,
                    "dot.status.directory",
                    "Directory status",
                    Some(status_data),
                );
            }
            OutputFormat::Text => {
                let home = dirs::home_dir().context("Failed to get home directory")?;
                let relative_dir = target_path.strip_prefix(&home).unwrap_or(&target_path);
                let tilde_dir = format!("~/{}", relative_dir.display());
                println!("{}", tilde_dir.bold());

                for (path, dotfile) in matching {
                    let status = get_dotfile_status(dotfile, db);
                    let repo_name = get_repo_name_for_dotfile(dotfile, cfg);
                    let dotfile_dir = get_dotfile_dir_name(dotfile, cfg);
                    let relative_path = path.strip_prefix(&home).unwrap_or(path);
                    let tilde_path = format!("~/{}", relative_path.display());
                    println!("  {} -> {}", tilde_path, status);
                    println!("    Source: {}", dotfile.source_path.display());
                    println!("    Repo: {repo_name} ({dotfile_dir})");
                }
            }
        }

        return Ok(());
    }

    match get_output_format() {
        OutputFormat::Json => {
            if let Some(dotfile) = all_dotfiles.get(&target_path) {
                let repo_name = get_repo_name_for_dotfile(dotfile, cfg);
                let dotfile_dir = get_dotfile_dir_name(dotfile, cfg);
                let status_data = serde_json::json!({
                    "path": target_path.display().to_string(),
                    "status": get_dotfile_status(dotfile, db),
                    "source": dotfile.source_path.display().to_string(),
                    "repo": repo_name.as_str(),
                    "dotfile_dir": dotfile_dir,
                    "tracked": true
                });
                emit(
                    Level::Info,
                    "dot.status.file",
                    "File status",
                    Some(status_data),
                );
            } else {
                let status_data = serde_json::json!({
                    "path": target_path.display().to_string(),
                    "tracked": false
                });
                emit(
                    Level::Info,
                    "dot.status.file",
                    "File not tracked",
                    Some(status_data),
                );
            }
        }
        OutputFormat::Text => {
            if let Some(dotfile) = all_dotfiles.get(&target_path) {
                let repo_name = get_repo_name_for_dotfile(dotfile, cfg);
                let dotfile_dir = get_dotfile_dir_name(dotfile, cfg);
                println!(
                    "{} -> {}",
                    target_path.display(),
                    get_dotfile_status(dotfile, db)
                );
                println!("  Source: {}", dotfile.source_path.display());
                println!("  Repo: {repo_name} ({dotfile_dir})");
            } else {
                println!("{} -> not tracked", target_path.display());
            }
        }
    }

    Ok(())
}

/// Show comprehensive status summary
pub fn show_status_summary(
    all_dotfiles: &HashMap<PathBuf, crate::dot::Dotfile>,
    cfg: &config::Config,
    db: &crate::dot::db::Database,
    show_all: bool,
) -> Result<()> {
    let home = dirs::home_dir().context("Failed to get home directory")?;

    // Categorize files and get summary
    let (files_by_status, summary) = categorize_files_and_get_summary(all_dotfiles, cfg, db);

    match get_output_format() {
        OutputFormat::Json => show_json_status(&files_by_status, &summary, show_all),
        OutputFormat::Text => show_text_status(&files_by_status, &summary, show_all, &home),
    }

    Ok(())
}

/// Categorize files by status and get summary statistics
pub fn categorize_files_and_get_summary<'a>(
    all_dotfiles: &'a HashMap<PathBuf, crate::dot::Dotfile>,
    cfg: &'a config::Config,
    db: &'a crate::dot::db::Database,
) -> (HashMap<DotFileStatus, Vec<FileInfo>>, StatusSummary) {
    let mut files_by_status = HashMap::new();
    let mut clean_count = 0;
    let mut modified_count = 0;
    let mut outdated_count = 0;

    // Load override config to check for overridden files
    let overrides = crate::dot::override_config::OverrideConfig::load().unwrap_or_default();

    for (target_path, dotfile) in all_dotfiles {
        let status = get_dotfile_status(dotfile, db);
        let repo_name = get_repo_name_for_dotfile(dotfile, cfg);
        let dotfile_dir = get_dotfile_dir_name(dotfile, cfg);
        let is_overridden = overrides.has_override(target_path);

        let file_info = FileInfo {
            target_path: target_path.clone(),
            dotfile: dotfile.clone(),
            repo_name: repo_name.clone(),
            dotfile_dir: dotfile_dir.clone(),
            is_overridden,
        };

        files_by_status
            .entry(status)
            .or_insert_with(Vec::new)
            .push(file_info);

        match status {
            DotFileStatus::Clean => clean_count += 1,
            DotFileStatus::Modified => modified_count += 1,
            DotFileStatus::Outdated => outdated_count += 1,
        }
    }

    // Sort files by path within each status category for better readability
    for files in files_by_status.values_mut() {
        files.sort_by(|a, b| a.target_path.cmp(&b.target_path));
    }

    let summary = StatusSummary {
        total_files: all_dotfiles.len(),
        clean_count,
        modified_count,
        outdated_count,
    };

    (files_by_status, summary)
}

/// Show status in JSON format
fn show_json_status(
    files_by_status: &HashMap<DotFileStatus, Vec<FileInfo>>,
    summary: &StatusSummary,
    show_all: bool,
) {
    let home = dirs::home_dir().unwrap_or_default();

    let modified_files: Vec<_> = files_by_status
        .get(&DotFileStatus::Modified)
        .unwrap_or(&vec![])
        .iter()
        .map(|file_info| {
            let relative_path = file_info
                .target_path
                .strip_prefix(&home)
                .unwrap_or(&file_info.target_path);
            serde_json::json!({
                "path": format!("~/{}", relative_path.display()),
                "status": "modified",
                "repo": file_info.repo_name.as_str(),
                "dotfile_dir": file_info.dotfile_dir
            })
        })
        .collect();

    let outdated_files: Vec<_> = files_by_status
        .get(&DotFileStatus::Outdated)
        .unwrap_or(&vec![])
        .iter()
        .map(|file_info| {
            let relative_path = file_info
                .target_path
                .strip_prefix(&home)
                .unwrap_or(&file_info.target_path);
            serde_json::json!({
                "path": format!("~/{}", relative_path.display()),
                "status": "outdated",
                "repo": file_info.repo_name.as_str(),
                "dotfile_dir": file_info.dotfile_dir
            })
        })
        .collect();

    let clean_files: Vec<_> = if show_all {
        files_by_status
            .get(&DotFileStatus::Clean)
            .unwrap_or(&vec![])
            .iter()
            .map(|file_info| {
                let relative_path = file_info
                    .target_path
                    .strip_prefix(&home)
                    .unwrap_or(&file_info.target_path);
                serde_json::json!({
                    "path": format!("~/{}", relative_path.display()),
                    "status": "clean",
                    "repo": file_info.repo_name.as_str(),
                    "dotfile_dir": file_info.dotfile_dir
                })
            })
            .collect()
    } else {
        vec![]
    };

    let status_data = serde_json::json!({
        "total_files": summary.total_files,
        "clean_count": summary.clean_count,
        "modified_count": summary.modified_count,
        "outdated_count": summary.outdated_count,
        "modified_files": modified_files,
        "outdated_files": outdated_files,
        "clean_files": clean_files,
        "show_all": show_all
    });

    emit(
        Level::Info,
        "dot.status.summary",
        "Dotfile status summary",
        Some(status_data),
    );
}

/// Show status in text format
fn show_text_status(
    files_by_status: &HashMap<DotFileStatus, Vec<FileInfo>>,
    summary: &StatusSummary,
    show_all: bool,
    home: &PathBuf,
) {
    // Show summary
    println!("Total tracked: {} files", summary.total_files);
    println!("{} Clean: {} files", "✓".green(), summary.clean_count);

    if summary.modified_count > 0 {
        println!(
            "{} Modified: {} files",
            "".yellow(),
            summary.modified_count
        );
    }

    if summary.outdated_count > 0 {
        println!("{} Outdated: {} files", "↓".blue(), summary.outdated_count);
    }

    // Show files with issues
    if summary.modified_count > 0 || summary.outdated_count > 0 {
        println!();

        if let Some(modified_files) = files_by_status.get(&DotFileStatus::Modified) {
            show_modified_files(modified_files, home);
        }

        if let Some(outdated_files) = files_by_status.get(&DotFileStatus::Outdated) {
            show_outdated_files(outdated_files, home);
        }
    }

    // Show all files if requested
    if show_all
        && summary.clean_count > 0
        && let Some(clean_files) = files_by_status.get(&DotFileStatus::Clean)
    {
        show_clean_files(clean_files, home);
    }

    // Show action suggestions
    show_action_suggestions(
        summary.modified_count,
        summary.outdated_count,
        summary.clean_count,
    );
}

/// Show modified files section
fn show_modified_files(files: &[FileInfo], home: &PathBuf) {
    println!("{}", " Modified files:".yellow().bold());
    for file_info in files {
        let relative_path = file_info
            .target_path
            .strip_prefix(home)
            .unwrap_or(&file_info.target_path);
        let tilde_path = format!("~/{}", relative_path.display());
        let override_indicator = if file_info.is_overridden { " [override]" } else { "" };
        println!(
            "  {} -> {} ({}: {}{})",
            tilde_path,
            "modified".yellow(),
            file_info.repo_name,
            file_info.dotfile_dir,
            override_indicator.magenta()
        );
    }
    println!();
}

/// Show outdated files section
fn show_outdated_files(files: &[FileInfo], home: &PathBuf) {
    println!("{}", "Outdated files:".blue().bold());
    for file_info in files {
        let relative_path = file_info
            .target_path
            .strip_prefix(home)
            .unwrap_or(&file_info.target_path);
        let tilde_path = format!("~/{}", relative_path.display());
        let override_indicator = if file_info.is_overridden { " [override]" } else { "" };
        println!(
            "  {} -> {} ({}: {}{})",
            tilde_path,
            "outdated".blue(),
            file_info.repo_name,
            file_info.dotfile_dir,
            override_indicator.magenta()
        );
    }
    println!();
}

/// Show clean files section
fn show_clean_files(files: &[FileInfo], home: &PathBuf) {
    println!("{}", "Clean files:".green().bold());
    for file_info in files {
        let relative_path = file_info
            .target_path
            .strip_prefix(home)
            .unwrap_or(&file_info.target_path);
        let tilde_path = format!("~/{}", relative_path.display());
        let override_indicator = if file_info.is_overridden { " [override]" } else { "" };
        println!(
            "  {} -> {} ({}: {}{})",
            tilde_path,
            "clean".green(),
            file_info.repo_name,
            file_info.dotfile_dir,
            override_indicator.magenta()
        );
    }
    println!();
}

/// Show action suggestions based on file status counts
fn show_action_suggestions(modified_count: usize, outdated_count: usize, clean_count: usize) {
    match get_output_format() {
        OutputFormat::Json => {
            let bin = env!("CARGO_BIN_NAME");
            let mut suggestions = Vec::new();

            if modified_count > 0 || outdated_count > 0 {
                if modified_count > 0 {
                    suggestions.push(format!(
                        "Use '{bin} dot apply' to apply changes from repositories"
                    ));
                    suggestions.push(format!(
                        "Use '{bin} dot add' to save your modifications to repositories"
                    ));
                    suggestions.push(format!(
                        "Use '{bin} dot reset <path>' to discard local changes"
                    ));
                }
                if outdated_count > 0 {
                    suggestions.push(format!(
                        "Use '{bin} dot reset <path>' to restore files to their original state"
                    ));
                }
                suggestions.push(format!(
                    "Use '{bin} dot status --all' to see all tracked files including clean ones"
                ));
            } else if clean_count > 0 {
                emit(
                    Level::Info,
                    "dot.status.message",
                    "All dotfiles are clean and up to date",
                    Some(serde_json::json!({
                        "status": "clean",
                        "message": "All dotfiles are clean and up to date!"
                    })),
                );
                return;
            } else {
                suggestions.push(format!(
                    "Use '{bin} dot repo clone <url>' to clone a repository"
                ));
            }

            let suggestion_data = serde_json::json!({
                "has_issues": modified_count > 0 || outdated_count > 0,
                "suggestions": suggestions
            });

            emit(
                Level::Info,
                "dot.status.suggestions",
                "Action suggestions",
                Some(suggestion_data),
            );
        }
        OutputFormat::Text => {
            let bin = env!("CARGO_BIN_NAME");
            if modified_count > 0 || outdated_count > 0 {
                println!("{}", "Suggested actions:".bold());
                if modified_count > 0 {
                    println!("  Use '{bin} dot apply' to apply changes from repositories");
                    println!("  Use '{bin} dot add' to save your modifications to repositories");
                    println!("  Use '{bin} dot reset <path>' to discard local changes");
                }
                if outdated_count > 0 {
                    println!(
                        "  Use '{bin} dot reset <path>' to restore files to their original state"
                    );
                }
                println!(
                    "  Use '{bin} dot status --all' to see all tracked files including clean ones"
                );
            } else if clean_count > 0 {
                println!("✓ All dotfiles are clean and up to date!");
            } else {
                println!(
                    "No dotfiles found. Use '{bin} dot repo clone <url>' to clone a repository."
                );
            }
        }
    }
}

/// Get the status of a dotfile
///
/// Returns:
/// - `Modified`: Target file exists and has been modified by user (doesn't match any known source hash)
/// - `Outdated`: Target file doesn't exist, or exists but doesn't match current source content
/// - `Clean`: Target file exists and matches current source content (or was created by instantCLI)
///
/// Note: Files that don't exist in the home directory but exist in the dotfile repository
/// are correctly classified as "Outdated" because they need to be applied.
pub fn get_dotfile_status(
    dotfile: &crate::dot::Dotfile,
    db: &crate::dot::db::Database,
) -> DotFileStatus {
    if !dotfile.is_target_unmodified(db).unwrap_or(false) {
        DotFileStatus::Modified
    } else if dotfile.is_outdated(db) {
        DotFileStatus::Outdated
    } else {
        DotFileStatus::Clean
    }
}
