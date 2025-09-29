use crate::dot::config;
use crate::dot::git::{get_dotfile_dir_name, get_repo_name_for_dotfile};
use crate::ui::{OutputFormat, get_output_format, info_with_data};
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
            DotFileStatus::Modified => write!(f, "{}", " modified".yellow()),
            DotFileStatus::Outdated => write!(f, "{}", "outdated".blue()),
            DotFileStatus::Clean => write!(f, "{}", "clean".green()),
        }
    }
}

/// File information with metadata for display
pub struct FileInfo {
    pub target_path: PathBuf,
    pub dotfile: crate::dot::Dotfile,
    pub repo_name: crate::dot::RepoName,
    pub dotfile_dir: String,
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
                info_with_data("dot.status.file", "File status", status_data);
            } else {
                let status_data = serde_json::json!({
                    "path": target_path.display().to_string(),
                    "tracked": false
                });
                info_with_data("dot.status.file", "File not tracked", status_data);
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
) -> (
    HashMap<DotFileStatus, Vec<FileInfo>>,
    StatusSummary,
) {
    let mut files_by_status = HashMap::new();
    let mut clean_count = 0;
    let mut modified_count = 0;
    let mut outdated_count = 0;

    for (target_path, dotfile) in all_dotfiles {
        let status = get_dotfile_status(dotfile, db);
        let repo_name = get_repo_name_for_dotfile(dotfile, cfg);
        let dotfile_dir = get_dotfile_dir_name(dotfile, cfg);

        let file_info = FileInfo {
            target_path: target_path.clone(),
            dotfile: dotfile.clone(),
            repo_name: repo_name.clone(),
            dotfile_dir: dotfile_dir.clone(),
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
            let relative_path = file_info.target_path.strip_prefix(&home).unwrap_or(&file_info.target_path);
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
            let relative_path = file_info.target_path.strip_prefix(&home).unwrap_or(&file_info.target_path);
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
                let relative_path = file_info.target_path.strip_prefix(&home).unwrap_or(&file_info.target_path);
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

    info_with_data("dot.status.summary", "Dotfile status summary", status_data);
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
        println!("{} Modified: {} files", "".yellow(), summary.modified_count);
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
    if show_all && summary.clean_count > 0 {
        if let Some(clean_files) = files_by_status.get(&DotFileStatus::Clean) {
            show_clean_files(clean_files, home);
        }
    }

    // Show action suggestions
    show_action_suggestions(summary.modified_count, summary.outdated_count, summary.clean_count);
}

/// Show modified files section
fn show_modified_files(files: &[FileInfo], home: &PathBuf) {
    println!("{}", " Modified files:".yellow().bold());
    for file_info in files {
        let relative_path = file_info.target_path.strip_prefix(home).unwrap_or(&file_info.target_path);
        let tilde_path = format!("~/{}", relative_path.display());
        println!(
            "  {} -> {} ({}: {})",
            tilde_path,
            "modified".yellow(),
            file_info.repo_name,
            file_info.dotfile_dir
        );
    }
    println!();
}

/// Show outdated files section
fn show_outdated_files(files: &[FileInfo], home: &PathBuf) {
    println!("{}", "Outdated files:".blue().bold());
    for file_info in files {
        let relative_path = file_info.target_path.strip_prefix(home).unwrap_or(&file_info.target_path);
        let tilde_path = format!("~/{}", relative_path.display());
        println!(
            "  {} -> {} ({}: {})",
            tilde_path,
            "outdated".blue(),
            file_info.repo_name,
            file_info.dotfile_dir
        );
    }
    println!();
}

/// Show clean files section
fn show_clean_files(files: &[FileInfo], home: &PathBuf) {
    println!("{}", "Clean files:".green().bold());
    for file_info in files {
        let relative_path = file_info.target_path.strip_prefix(home).unwrap_or(&file_info.target_path);
        let tilde_path = format!("~/{}", relative_path.display());
        println!(
            "  {} -> {} ({}: {})",
            tilde_path,
            "clean".green(),
            file_info.repo_name,
            file_info.dotfile_dir
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
                        "Use '{bin} dot fetch' to save your modifications to repositories"
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
                info_with_data(
                    "dot.status.message",
                    "All dotfiles are clean and up to date",
                    serde_json::json!({
                        "status": "clean",
                        "message": "All dotfiles are clean and up to date!"
                    }),
                );
                return;
            } else {
                suggestions.push(format!(
                    "Use '{bin} dot repo add <url>' to add a repository"
                ));
            }

            let suggestion_data = serde_json::json!({
                "has_issues": modified_count > 0 || outdated_count > 0,
                "suggestions": suggestions
            });

            info_with_data(
                "dot.status.suggestions",
                "Action suggestions",
                suggestion_data,
            );
        }
        OutputFormat::Text => {
            let bin = env!("CARGO_BIN_NAME");
            if modified_count > 0 || outdated_count > 0 {
                println!("{}", "Suggested actions:".bold());
                if modified_count > 0 {
                    println!("  Use '{bin} dot apply' to apply changes from repositories");
                    println!("  Use '{bin} dot fetch' to save your modifications to repositories");
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
                println!("No dotfiles found. Use '{bin} dot repo add <url>' to add a repository.");
            }
        }
    }
}

/// Get the status of a dotfile
pub fn get_dotfile_status(dotfile: &crate::dot::Dotfile, db: &crate::dot::db::Database) -> DotFileStatus {
    if !dotfile.is_target_unmodified(db).unwrap_or(false) {
        DotFileStatus::Modified
    } else if dotfile.is_outdated(db) {
        DotFileStatus::Outdated
    } else {
        DotFileStatus::Clean
    }
}