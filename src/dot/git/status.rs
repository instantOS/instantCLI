use crate::dot::config::DotfileConfig;
use crate::dot::encryption::classify_encrypted_failure;
use crate::dot::git::{get_dotfile_dir_name, get_repo_name_for_dotfile};
use crate::dot::units::UnitIndex;
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
    IdentityRequired,
    EncryptedError,
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
            DotFileStatus::IdentityRequired => write!(
                f,
                "{} {}",
                crate::ui::nerd_font::NerdFont::ShieldAlert
                    .to_string()
                    .yellow(),
                "encrypted: identity required".yellow()
            ),
            DotFileStatus::EncryptedError => write!(
                f,
                "{} {}",
                crate::ui::nerd_font::NerdFont::Warning.to_string().red(),
                "encrypted: processing error".red()
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
    pub identity_required_count: usize,
    pub encrypted_error_count: usize,
}

/// Show status for a single file
pub fn show_single_file_status(
    path_str: &str,
    all_dotfiles: &HashMap<PathBuf, crate::dot::Dotfile>,
    cfg: &DotfileConfig,
    db: &crate::dot::db::Database,
    _show_sources: bool,
    unit_index: &UnitIndex,
    include_root: bool,
) -> Result<()> {
    let target_path = crate::dot::resolve_dotfile_path(path_str, include_root, true)?;
    let home = dirs::home_dir().context("Failed to get home directory")?;

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

        matching.sort_by_key(|(a, _)| a.as_path());

        match get_output_format() {
            OutputFormat::Json => {
                let files: Vec<_> = matching
                    .into_iter()
                    .map(|(path, dotfile)| {
                        let status = get_dotfile_status(dotfile, db, unit_index);
                        let repo_name = get_repo_name_for_dotfile(dotfile, cfg);
                        let dotfile_dir = get_dotfile_dir_name(dotfile, cfg);
                        let path_display = crate::dot::display_path(path, dotfile.is_root);
                        serde_json::json!({
                            "path": path_display,
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
                let dir_display = crate::dot::display_path(&target_path, false);
                println!("{}", dir_display.bold());

                for (path, dotfile) in matching {
                    let status = get_dotfile_status(dotfile, db, unit_index);
                    let repo_name = get_repo_name_for_dotfile(dotfile, cfg);
                    let dotfile_dir = get_dotfile_dir_name(dotfile, cfg);
                    let path_display = crate::dot::display_path(path, dotfile.is_root);

                    let override_indicator = if let Ok(overrides) =
                        crate::dot::override_config::OverrideConfig::load()
                    {
                        if overrides.get_override(path).is_some() {
                            " [override]"
                        } else {
                            ""
                        }
                    } else {
                        ""
                    };

                    println!("  {} -> {}", path_display, status);
                    println!("    Source: {}", dotfile.source_path.display());
                    println!("    Repo: {repo_name} ({dotfile_dir}){override_indicator}");
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
                let status = get_dotfile_status(dotfile, db, unit_index);
                let unit_details = unit_index
                    .unit_statuses_for_target(&target_path)
                    .into_iter()
                    .map(|unit_status| {
                        let unit_display = format!("~/{}", unit_status.unit_path.display());
                        let modified_display: Vec<_> = unit_status
                            .modified_files
                            .iter()
                            .map(|path| {
                                let relative_path = path.strip_prefix(&home).unwrap_or(path);
                                format!("~/{}", relative_path.display())
                            })
                            .collect();
                        serde_json::json!({
                            "path": unit_display,
                            "modified_files": modified_display,
                            "modified": !unit_status.modified_files.is_empty()
                        })
                    })
                    .collect::<Vec<_>>();
                let mut status_data = serde_json::json!({
                    "path": target_path.display().to_string(),
                    "status": status,
                    "source": dotfile.source_path.display().to_string(),
                    "repo": repo_name.as_str(),
                    "dotfile_dir": dotfile_dir,
                    "tracked": true
                });
                status_data["units"] = serde_json::Value::Array(unit_details);
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

                // Check for override
                let override_indicator =
                    if let Ok(overrides) = crate::dot::override_config::OverrideConfig::load() {
                        if overrides.get_override(&target_path).is_some() {
                            " [override]"
                        } else {
                            ""
                        }
                    } else {
                        ""
                    };

                println!(
                    "{} -> {}",
                    target_path.display(),
                    get_dotfile_status(dotfile, db, unit_index)
                );
                println!("  Source: {}", dotfile.source_path.display());
                println!("  Repo: {repo_name} ({dotfile_dir}){override_indicator}");

                let unit_statuses = unit_index.unit_statuses_for_target(&target_path);
                if !unit_statuses.is_empty() {
                    println!("  Units:");
                    for unit_status in unit_statuses {
                        let unit_display = format!("~/{}", unit_status.unit_path.display());
                        if unit_status.modified_files.is_empty() {
                            println!("    {} (clean)", unit_display.green());
                            continue;
                        }

                        println!(
                            "    {} (modified files: {})",
                            unit_display.yellow(),
                            unit_status.modified_files.len()
                        );
                        for path in unit_status.modified_files {
                            let relative_path = path.strip_prefix(&home).unwrap_or(&path);
                            let tilde_path = format!("~/{}", relative_path.display());
                            println!("      - {}", tilde_path.yellow());
                        }
                    }
                }
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
    cfg: &DotfileConfig,
    db: &crate::dot::db::Database,
    show_all: bool,
    show_sources: bool,
    unit_index: &UnitIndex,
    _include_root: bool,
) -> Result<()> {
    let home = dirs::home_dir().context("Failed to get home directory")?;

    // Report each file's own state separately from the effective protection state of its unit.
    // A modified unit still protects all of its members during apply, but should not make every
    // unchanged member look individually modified in the status summary.
    let (files_by_status, summary) =
        categorize_files_and_get_summary(all_dotfiles, cfg, db, &UnitIndex::default());

    match get_output_format() {
        OutputFormat::Json => show_json_status(
            &files_by_status,
            &summary,
            show_all,
            show_sources,
            cfg,
            unit_index,
        ),
        OutputFormat::Text => show_text_status(
            &files_by_status,
            &summary,
            show_all,
            show_sources,
            &home,
            cfg,
            unit_index,
        ),
    }

    Ok(())
}

/// Categorize files by status and get summary statistics
pub fn categorize_files_and_get_summary<'a>(
    all_dotfiles: &'a HashMap<PathBuf, crate::dot::Dotfile>,
    cfg: &'a DotfileConfig,
    db: &'a crate::dot::db::Database,
    unit_index: &'a UnitIndex,
) -> (HashMap<DotFileStatus, Vec<FileInfo>>, StatusSummary) {
    let mut files_by_status = HashMap::new();
    let mut clean_count = 0;
    let mut modified_count = 0;
    let mut outdated_count = 0;
    let mut identity_required_count = 0;
    let mut encrypted_error_count = 0;

    // Load override config to check for overridden files
    let overrides = crate::dot::override_config::OverrideConfig::load().unwrap_or_default();

    for (target_path, dotfile) in all_dotfiles {
        let status = get_dotfile_status(dotfile, db, unit_index);
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
            DotFileStatus::IdentityRequired => identity_required_count += 1,
            DotFileStatus::EncryptedError => encrypted_error_count += 1,
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
        identity_required_count,
        encrypted_error_count,
    };

    (files_by_status, summary)
}

/// Show status in JSON format
fn show_json_status(
    files_by_status: &HashMap<DotFileStatus, Vec<FileInfo>>,
    summary: &StatusSummary,
    show_all: bool,
    show_sources: bool,
    cfg: &DotfileConfig,
    unit_index: &UnitIndex,
) {
    let home = dirs::home_dir().unwrap_or_default();

    // Helper function to get priority number for a repo
    let get_priority = |repo_name: &str| -> usize {
        cfg.repos
            .iter()
            .position(|r| r.name == repo_name)
            .map(|p| p + 1)
            .unwrap_or(0)
    };

    let modified_files: Vec<_> = files_by_status
        .get(&DotFileStatus::Modified)
        .unwrap_or(&vec![])
        .iter()
        .map(|file_info| {
            let relative_path = file_info
                .target_path
                .strip_prefix(&home)
                .unwrap_or(&file_info.target_path);
            let priority = get_priority(file_info.repo_name.as_str());
            let mut json_val = serde_json::json!({
                "path": format!("~/{}", relative_path.display()),
                "status": "modified",
                "repo": file_info.repo_name.as_str(),
                "dotfile_dir": file_info.dotfile_dir
            });
            if show_sources {
                json_val["priority"] = serde_json::json!(priority);
                json_val["override"] = serde_json::json!(file_info.is_overridden);
            }
            json_val
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
            let priority = get_priority(file_info.repo_name.as_str());
            let mut json_val = serde_json::json!({
                "path": format!("~/{}", relative_path.display()),
                "status": "outdated",
                "repo": file_info.repo_name.as_str(),
                "dotfile_dir": file_info.dotfile_dir
            });
            if show_sources {
                json_val["priority"] = serde_json::json!(priority);
                json_val["override"] = serde_json::json!(file_info.is_overridden);
            }
            json_val
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
                let priority = get_priority(file_info.repo_name.as_str());
                let mut json_val = serde_json::json!({
                    "path": format!("~/{}", relative_path.display()),
                    "status": "clean",
                    "repo": file_info.repo_name.as_str(),
                    "dotfile_dir": file_info.dotfile_dir
                });
                if show_sources {
                    json_val["priority"] = serde_json::json!(priority);
                    json_val["override"] = serde_json::json!(file_info.is_overridden);
                }
                json_val
            })
            .collect()
    } else {
        vec![]
    };

    let identity_required_files: Vec<_> = files_by_status
        .get(&DotFileStatus::IdentityRequired)
        .unwrap_or(&vec![])
        .iter()
        .map(|file_info| {
            let relative_path = file_info
                .target_path
                .strip_prefix(&home)
                .unwrap_or(&file_info.target_path);
            let priority = get_priority(file_info.repo_name.as_str());
            let mut json_val = serde_json::json!({
                "path": format!("~/{}", relative_path.display()),
                "status": "identity_required",
                "repo": file_info.repo_name.as_str(),
                "dotfile_dir": file_info.dotfile_dir,
                "reason": "encrypted_source"
            });
            if show_sources {
                json_val["priority"] = serde_json::json!(priority);
                json_val["override"] = serde_json::json!(file_info.is_overridden);
            }
            json_val
        })
        .collect();

    let encrypted_error_files: Vec<_> = files_by_status
        .get(&DotFileStatus::EncryptedError)
        .unwrap_or(&vec![])
        .iter()
        .map(|file_info| {
            let relative_path = file_info
                .target_path
                .strip_prefix(&home)
                .unwrap_or(&file_info.target_path);
            let priority = get_priority(file_info.repo_name.as_str());
            let mut json_val = serde_json::json!({
                "path": format!("~/{}", relative_path.display()),
                "status": "encrypted_error",
                "repo": file_info.repo_name.as_str(),
                "dotfile_dir": file_info.dotfile_dir,
                "reason": "encrypted_source_error"
            });
            if show_sources {
                json_val["priority"] = serde_json::json!(priority);
                json_val["override"] = serde_json::json!(file_info.is_overridden);
            }
            json_val
        })
        .collect();

    let modified_units: Vec<_> = unit_index
        .modified_unit_statuses()
        .into_iter()
        .map(|unit_status| {
            let modified_files: Vec<_> = unit_status
                .modified_files
                .iter()
                .map(|path| {
                    let relative_path = path.strip_prefix(&home).unwrap_or(path);
                    format!("~/{}", relative_path.display())
                })
                .collect();
            serde_json::json!({
                "path": format!("~/{}", unit_status.unit_path.display()),
                "status": "modified",
                "modified_files": modified_files
            })
        })
        .collect();

    let status_data = serde_json::json!({
        "total_files": summary.total_files,
        "clean_count": summary.clean_count,
        "modified_count": summary.modified_count,
        "modified_unit_count": modified_units.len(),
        "outdated_count": summary.outdated_count,
        "identity_required_count": summary.identity_required_count,
        "encrypted_error_count": summary.encrypted_error_count,
        "modified_files": modified_files,
        "modified_units": modified_units,
        "outdated_files": outdated_files,
        "identity_required_files": identity_required_files,
        "encrypted_error_files": encrypted_error_files,
        "clean_files": clean_files,
        "show_all": show_all,
        "show_sources": show_sources
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
    show_sources: bool,
    home: &PathBuf,
    cfg: &DotfileConfig,
    unit_index: &UnitIndex,
) {
    // Helper function to get priority number for a repo
    let get_priority = |repo_name: &str| -> usize {
        cfg.repos
            .iter()
            .position(|r| r.name == repo_name)
            .map(|p| p + 1)
            .unwrap_or(0)
    };

    // Show summary
    println!("Total tracked: {} files", summary.total_files);
    println!(
        "{} Clean: {} files",
        char::from(NerdFont::Check).to_string().green(),
        summary.clean_count
    );

    if summary.modified_count > 0 {
        println!(
            "{} Modified: {} files",
            char::from(NerdFont::Edit).to_string().yellow(),
            summary.modified_count
        );
    }

    let modified_units = unit_index.modified_unit_statuses();
    if !modified_units.is_empty() {
        let label = if modified_units.len() == 1 {
            "unit"
        } else {
            "units"
        };
        println!(
            "{} Modified units: {} {}",
            char::from(NerdFont::Edit).to_string().yellow(),
            modified_units.len(),
            label
        );
    }

    if summary.outdated_count > 0 {
        println!(
            "{} Outdated: {} files",
            char::from(NerdFont::ArrowDown).to_string().blue(),
            summary.outdated_count
        );
    }

    if summary.identity_required_count > 0 {
        println!(
            "{} Encrypted: {} files need an encryption key",
            crate::ui::nerd_font::NerdFont::ShieldAlert
                .to_string()
                .yellow(),
            summary.identity_required_count
        );
    }

    if summary.encrypted_error_count > 0 {
        println!(
            "{} Encrypted: {} files have non-identity decryption errors",
            crate::ui::nerd_font::NerdFont::Warning.to_string().red(),
            summary.encrypted_error_count
        );
    }

    // Show files with issues
    if summary.modified_count > 0
        || !modified_units.is_empty()
        || summary.outdated_count > 0
        || summary.identity_required_count > 0
        || summary.encrypted_error_count > 0
    {
        println!();

        if !modified_units.is_empty() {
            show_modified_units(&modified_units, home);
        }

        if let Some(modified_files) = files_by_status.get(&DotFileStatus::Modified) {
            show_modified_files(
                modified_files,
                home,
                show_sources,
                &get_priority,
                unit_index,
            );
        }

        if let Some(outdated_files) = files_by_status.get(&DotFileStatus::Outdated) {
            show_outdated_files(outdated_files, home, show_sources, &get_priority);
        }

        if let Some(identity_required_files) = files_by_status.get(&DotFileStatus::IdentityRequired)
        {
            show_identity_required_files(
                identity_required_files,
                home,
                show_sources,
                &get_priority,
            );
        }

        if let Some(encrypted_error_files) = files_by_status.get(&DotFileStatus::EncryptedError) {
            show_encrypted_error_files(encrypted_error_files, home, show_sources, &get_priority);
        }
    }

    // Show all files if requested
    if show_all
        && summary.clean_count > 0
        && let Some(clean_files) = files_by_status.get(&DotFileStatus::Clean)
    {
        show_clean_files(clean_files, home, show_sources, &get_priority);
    }

    // Show action suggestions
    show_action_suggestions(
        summary.modified_count,
        summary.outdated_count,
        summary.identity_required_count,
        summary.encrypted_error_count,
        summary.clean_count,
    );
}

/// Show modified files section
fn show_modified_files(
    files: &[FileInfo],
    home: &PathBuf,
    show_sources: bool,
    get_priority: &dyn Fn(&str) -> usize,
    unit_index: &UnitIndex,
) {
    let standalone_files: Vec<_> = files
        .iter()
        .filter(|file_info| !unit_index.is_target_in_modified_unit(&file_info.target_path))
        .collect();
    if standalone_files.is_empty() {
        return;
    }

    println!("{}", " Standalone modified files:".yellow().bold());
    for file_info in standalone_files {
        let relative_path = file_info
            .target_path
            .strip_prefix(home)
            .unwrap_or(&file_info.target_path);
        let tilde_path = format!("~/{}", relative_path.display());
        let override_indicator = if file_info.is_overridden {
            " [override]"
        } else {
            ""
        };

        if show_sources {
            let priority = get_priority(file_info.repo_name.as_str());
            println!(
                "  {} → {} / {} (P{}){}",
                tilde_path,
                file_info.repo_name.as_str().bright_purple(),
                file_info.dotfile_dir,
                priority,
                override_indicator.magenta()
            );
        } else {
            println!(
                "  {} -> {} ({}: {}{})",
                tilde_path,
                "modified".yellow(),
                file_info.repo_name,
                file_info.dotfile_dir,
                override_indicator.magenta()
            );
        }
    }
    println!();
}

fn show_modified_units(modified_units: &[crate::dot::units::UnitStatus], home: &PathBuf) {
    println!("{}", " Modified units:".yellow().bold());
    for unit_status in modified_units {
        println!(
            "  {}",
            format!("~/{}", unit_status.unit_path.display()).yellow()
        );
        println!("    Modified files causing this unit state:");
        for path in &unit_status.modified_files {
            let relative_path = path.strip_prefix(home).unwrap_or(path);
            println!("      - ~/{}", relative_path.display());
        }
    }
    println!();
}

/// Show outdated files section
fn show_outdated_files(
    files: &[FileInfo],
    home: &PathBuf,
    show_sources: bool,
    get_priority: &dyn Fn(&str) -> usize,
) {
    println!("{}", "Outdated files:".blue().bold());
    for file_info in files {
        let relative_path = file_info
            .target_path
            .strip_prefix(home)
            .unwrap_or(&file_info.target_path);
        let tilde_path = format!("~/{}", relative_path.display());
        let override_indicator = if file_info.is_overridden {
            " [override]"
        } else {
            ""
        };

        if show_sources {
            let priority = get_priority(file_info.repo_name.as_str());
            println!(
                "  {} → {} / {} (P{}){}",
                tilde_path,
                file_info.repo_name.as_str().bright_purple(),
                file_info.dotfile_dir,
                priority,
                override_indicator.magenta()
            );
        } else {
            println!(
                "  {} -> {} ({}: {}{})",
                tilde_path,
                "outdated".blue(),
                file_info.repo_name,
                file_info.dotfile_dir,
                override_indicator.magenta()
            );
        }
    }
    println!();
}

fn show_identity_required_files(
    files: &[FileInfo],
    home: &PathBuf,
    show_sources: bool,
    get_priority: &dyn Fn(&str) -> usize,
) {
    println!("{}", "Encrypted files needing identity:".yellow().bold());
    for file_info in files {
        let relative_path = file_info
            .target_path
            .strip_prefix(home)
            .unwrap_or(&file_info.target_path);
        let tilde_path = format!("~/{}", relative_path.display());
        let override_indicator = if file_info.is_overridden {
            " [override]"
        } else {
            ""
        };

        if show_sources {
            let priority = get_priority(file_info.repo_name.as_str());
            println!(
                "  {} -> {} / {} (P{}){}",
                tilde_path,
                file_info.repo_name.as_str().bright_purple(),
                file_info.dotfile_dir,
                priority,
                override_indicator.magenta()
            );
        } else {
            println!(
                "  {} -> {} ({}: {}{})",
                tilde_path,
                "identity required".yellow(),
                file_info.repo_name,
                file_info.dotfile_dir,
                override_indicator.magenta()
            );
        }
    }
    println!();
}

fn show_encrypted_error_files(
    files: &[FileInfo],
    home: &PathBuf,
    show_sources: bool,
    get_priority: &dyn Fn(&str) -> usize,
) {
    println!("{}", "Encrypted files with errors:".red().bold());
    for file_info in files {
        let relative_path = file_info
            .target_path
            .strip_prefix(home)
            .unwrap_or(&file_info.target_path);
        let tilde_path = format!("~/{}", relative_path.display());
        let override_indicator = if file_info.is_overridden {
            " [override]"
        } else {
            ""
        };

        if show_sources {
            let priority = get_priority(file_info.repo_name.as_str());
            println!(
                "  {} -> {} / {} (P{}){}",
                tilde_path,
                file_info.repo_name.as_str().bright_purple(),
                file_info.dotfile_dir,
                priority,
                override_indicator.magenta()
            );
        } else {
            println!(
                "  {} -> {} ({}: {}{})",
                tilde_path,
                "encrypted error".red(),
                file_info.repo_name,
                file_info.dotfile_dir,
                override_indicator.magenta()
            );
        }
    }
    println!();
}

/// Show clean files section
fn show_clean_files(
    files: &[FileInfo],
    home: &PathBuf,
    show_sources: bool,
    get_priority: &dyn Fn(&str) -> usize,
) {
    println!("{}", "Clean files:".green().bold());
    for file_info in files {
        let relative_path = file_info
            .target_path
            .strip_prefix(home)
            .unwrap_or(&file_info.target_path);
        let tilde_path = format!("~/{}", relative_path.display());
        let override_indicator = if file_info.is_overridden {
            " [override]"
        } else {
            ""
        };

        if show_sources {
            let priority = get_priority(file_info.repo_name.as_str());
            println!(
                "  {} → {} / {} (P{}){}",
                tilde_path,
                file_info.repo_name.as_str().bright_purple(),
                file_info.dotfile_dir,
                priority,
                override_indicator.magenta()
            );
        } else {
            println!(
                "  {} -> {} ({}: {}{})",
                tilde_path,
                "clean".green(),
                file_info.repo_name,
                file_info.dotfile_dir,
                override_indicator.magenta()
            );
        }
    }
    println!();
}

/// Show action suggestions based on file status counts
fn show_action_suggestions(
    modified_count: usize,
    outdated_count: usize,
    identity_required_count: usize,
    encrypted_error_count: usize,
    clean_count: usize,
) {
    match get_output_format() {
        OutputFormat::Json => {
            let bin = env!("CARGO_BIN_NAME");
            let mut suggestions = Vec::new();

            if modified_count > 0
                || outdated_count > 0
                || identity_required_count > 0
                || encrypted_error_count > 0
            {
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
                if identity_required_count > 0 {
                    suggestions.push(format!(
                        "Configure an encryption key with '{bin} dot encrypt generate' or set $AGE_IDENTITY"
                    ));
                }
                if encrypted_error_count > 0 {
                    suggestions.push(format!(
                        "Use '{bin} dot diff <path>' to inspect encrypted source errors for affected files"
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
                "has_issues": modified_count > 0
                    || outdated_count > 0
                    || identity_required_count > 0
                    || encrypted_error_count > 0,
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
            if modified_count > 0
                || outdated_count > 0
                || identity_required_count > 0
                || encrypted_error_count > 0
            {
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
                if identity_required_count > 0 {
                    println!(
                        "  Configure an encryption key with '{bin} dot encrypt generate' or set $AGE_IDENTITY"
                    );
                }
                if encrypted_error_count > 0 {
                    println!(
                        "  Use '{bin} dot diff <path>' to inspect encrypted source errors for affected files"
                    );
                }
                println!(
                    "  Use '{bin} dot status --all' to see all tracked files including clean ones"
                );
            } else if clean_count > 0 {
                println!(
                    "{} All dotfiles are clean and up to date!",
                    char::from(NerdFont::Check).to_string().green()
                );
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
    unit_index: &UnitIndex,
) -> DotFileStatus {
    match dotfile.is_target_unmodified(db) {
        Ok(false) => return DotFileStatus::Modified,
        Ok(true) => {}
        Err(err) if dotfile.kind == crate::dot::dotfile::SourceKind::Age => {
            let reason = classify_encrypted_failure(&err);
            return if reason.is_identity_related() {
                DotFileStatus::IdentityRequired
            } else {
                DotFileStatus::EncryptedError
            };
        }
        Err(_) => return DotFileStatus::Modified,
    }

    if unit_index.is_target_in_modified_unit(&dotfile.target_path) {
        return DotFileStatus::Modified;
    }

    match dotfile.is_outdated(db) {
        Ok(true) => return DotFileStatus::Outdated,
        Ok(false) => {}
        Err(err) if dotfile.kind == crate::dot::dotfile::SourceKind::Age => {
            let reason = classify_encrypted_failure(&err);
            return if reason.is_identity_related() {
                DotFileStatus::IdentityRequired
            } else {
                DotFileStatus::EncryptedError
            };
        }
        Err(_) => return DotFileStatus::Modified,
    }

    DotFileStatus::Clean
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dot::db::{Database, DotFileType};
    use crate::dot::dotfile::Dotfile;
    use age::secrecy::ExposeSecret;
    use serial_test::serial;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn status_reports_known_unmodified_content_as_outdated_when_source_differs() {
        let dir = tempdir().unwrap();
        let source_path = dir.path().join("repo/dots/config.toml");
        let target_path = dir.path().join("home/config.toml");
        fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        fs::create_dir_all(target_path.parent().unwrap()).unwrap();
        fs::write(&source_path, "new source").unwrap();
        fs::write(&target_path, "previous source").unwrap();

        let source_file = fs::File::open(&source_path).unwrap();
        source_file
            .set_times(
                fs::FileTimes::new()
                    .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(100)),
            )
            .unwrap();
        let target_file = fs::File::open(&target_path).unwrap();
        target_file
            .set_times(
                fs::FileTimes::new()
                    .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(200)),
            )
            .unwrap();

        let db = Database::new(dir.path().join("test.db")).unwrap();
        let previous_hash = Dotfile::compute_hash(&target_path).unwrap();
        db.add_hash(
            &previous_hash,
            &dir.path().join("repo/dots/previous-config.toml"),
            DotFileType::SourceFile,
        )
        .unwrap();

        let dotfile = Dotfile::new(source_path, target_path, false);
        assert_eq!(
            get_dotfile_status(&dotfile, &db, &UnitIndex::default()),
            DotFileStatus::Outdated
        );
    }

    #[test]
    #[serial]
    fn summary_separates_modified_unit_from_unchanged_members() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        let config_home = dir.path().join("config");
        let source_dir = dir.path().join("repo/dots/.config/editor");
        let target_dir = home.join(".config/editor");
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(&target_dir).unwrap();
        fs::create_dir_all(&config_home).unwrap();

        let prev_home = std::env::var_os("HOME");
        let prev_xdg = std::env::var_os("XDG_CONFIG_HOME");
        // SAFETY: this test is serialised and restores the process env below.
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("XDG_CONFIG_HOME", &config_home);
        }

        let modified_source = source_dir.join("modified.lua");
        let modified_target = target_dir.join("modified.lua");
        fs::write(&modified_source, "repository version").unwrap();
        fs::write(&modified_target, "local version").unwrap();

        let clean_source = source_dir.join("clean.lua");
        let clean_target = target_dir.join("clean.lua");
        fs::write(&clean_source, "same version").unwrap();
        fs::write(&clean_target, "same version").unwrap();

        let db = Database::new(dir.path().join("test.db")).unwrap();
        let modified_dotfile = Dotfile::new(modified_source, modified_target.clone(), false);
        let clean_dotfile = Dotfile::new(clean_source, clean_target.clone(), false);
        let all_dotfiles = HashMap::from([
            (modified_target.clone(), modified_dotfile),
            (clean_target.clone(), clean_dotfile),
        ]);
        let unit_index = crate::dot::units::build_unit_index(
            &all_dotfiles,
            &[PathBuf::from(".config/editor")],
            &db,
        )
        .unwrap();
        let (_, summary) = categorize_files_and_get_summary(
            &all_dotfiles,
            &DotfileConfig::default(),
            &db,
            &UnitIndex::default(),
        );

        unsafe {
            match prev_home {
                Some(value) => std::env::set_var("HOME", value),
                None => std::env::remove_var("HOME"),
            }
            match prev_xdg {
                Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
                None => std::env::remove_var("XDG_CONFIG_HOME"),
            }
        }

        assert_eq!(summary.modified_count, 1);
        assert_eq!(summary.clean_count, 1);
        assert!(unit_index.is_target_in_modified_unit(&clean_target));
        let modified_units = unit_index.modified_unit_statuses();
        assert_eq!(modified_units.len(), 1);
        assert_eq!(modified_units[0].modified_files, vec![modified_target]);
    }

    #[test]
    #[serial]
    fn encrypted_status_reports_identity_required_on_decrypt_failure() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        let config_home = dir.path().join("config");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&config_home).unwrap();

        let prev_home = std::env::var_os("HOME");
        let prev_xdg = std::env::var_os("XDG_CONFIG_HOME");
        let prev_age = std::env::var_os("AGE_IDENTITY");
        // SAFETY: this test is serialised and restores the process env below.
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("XDG_CONFIG_HOME", &config_home);
            std::env::remove_var("AGE_IDENTITY");
        }

        let source_path = dir.path().join("repo/dots/secret.txt.age");
        let target_path = home.join("secret.txt");
        fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        fs::write(&source_path, "not an age file").unwrap();
        fs::write(&target_path, "plaintext").unwrap();

        let db = Database::new(dir.path().join("test.db")).unwrap();
        let dotfile = Dotfile::new(source_path, target_path, false);
        let status = get_dotfile_status(&dotfile, &db, &UnitIndex::default());

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

        assert_eq!(status, DotFileStatus::IdentityRequired);
    }

    #[test]
    #[serial]
    fn encrypted_status_reports_encrypted_error_for_invalid_ciphertext_with_identity() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        let config_home = dir.path().join("config");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&config_home).unwrap();

        let identity = age::x25519::Identity::generate();
        let identity_file = dir.path().join("identity.key");
        fs::write(&identity_file, identity.to_string().expose_secret()).unwrap();

        let prev_home = std::env::var_os("HOME");
        let prev_xdg = std::env::var_os("XDG_CONFIG_HOME");
        let prev_age = std::env::var_os("AGE_IDENTITY");
        // SAFETY: this test is serialised and restores the process env below.
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("XDG_CONFIG_HOME", &config_home);
            std::env::set_var("AGE_IDENTITY", &identity_file);
        }

        let source_path = dir.path().join("repo/dots/secret.txt.age");
        let target_path = home.join("secret.txt");
        fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        fs::write(&source_path, "not an age file").unwrap();
        fs::write(&target_path, "plaintext").unwrap();

        let db = Database::new(dir.path().join("test.db")).unwrap();
        let dotfile = Dotfile::new(source_path, target_path, false);
        let status = get_dotfile_status(&dotfile, &db, &UnitIndex::default());

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

        assert_eq!(status, DotFileStatus::EncryptedError);
    }
}
