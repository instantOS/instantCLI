use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use colored::Colorize;

use crate::dot::config::DotfileConfig;
use crate::dot::git::{get_dotfile_dir_name, get_repo_name_for_dotfile};
use crate::dot::units::UnitIndex;
use crate::ui::prelude::*;

use super::get_dotfile_status;

/// Show status for a single file
pub fn show_single_file_status(
    path_str: &str,
    all_dotfiles: &HashMap<PathBuf, crate::dot::Dotfile>,
    cfg: &DotfileConfig,
    db: &crate::dot::db::Database,
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
                let overrides =
                    crate::dot::override_config::OverrideConfig::load().unwrap_or_default();

                for (path, dotfile) in matching {
                    let status = get_dotfile_status(dotfile, db, unit_index);
                    let repo_name = get_repo_name_for_dotfile(dotfile, cfg);
                    let dotfile_dir = get_dotfile_dir_name(dotfile, cfg);
                    let path_display = crate::dot::display_path(path, dotfile.is_root);

                    let override_indicator = if overrides.get_override(path).is_some() {
                        " [override]"
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
