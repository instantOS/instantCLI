use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::dotfile::Dotfile;
use crate::dot::units::{find_unit_for_path, get_all_units, get_modified_units};
use crate::dot::utils::get_all_dotfiles;
use crate::ui::prelude::*;
use anyhow::Result;
use colored::Colorize;
use std::collections::HashSet;
use std::path::PathBuf;

/// Result of applying a single dotfile
#[derive(Debug, PartialEq)]
enum ApplyAction {
    /// File was created (didn't exist before)
    Created,
    /// File was updated (existed but was outdated)
    Updated,
    /// File was skipped (user modified)
    Skipped,
    /// File was skipped because another file in the same unit was modified
    SkippedUnit,
    /// File was already up-to-date (no action needed)
    AlreadyUpToDate,
}

/// Apply all dotfiles from configured repositories
pub fn apply_all(config: &Config, db: &Database) -> Result<()> {
    let all_dotfiles = get_all_dotfiles(config, db)?;
    let home = PathBuf::from(shellexpand::tilde("~").to_string());

    if all_dotfiles.is_empty() {
        emit(
            Level::Info,
            "dot.apply.no_dotfiles",
            &format!("{} No dotfiles configured", char::from(NerdFont::Info)),
            None,
        );
        return Ok(());
    }

    // Get all unit definitions and find which units have modified files
    let units = get_all_units(config, db)?;
    let modified_units = get_modified_units(&all_dotfiles, &units, db)?;

    // Track which modified units we've already reported
    let mut reported_units: HashSet<PathBuf> = HashSet::new();

    let mut created_files = Vec::new();
    let mut updated_files = Vec::new();
    let mut skipped_files = Vec::new();
    let mut skipped_unit_count = 0;
    let mut unchanged_count = 0;

    // Apply each dotfile and track the action taken
    for dotfile in all_dotfiles.values() {
        // Check if this file belongs to a modified unit
        let in_modified_unit = find_unit_for_path(&dotfile.target_path, &units)
            .map(|unit_path| modified_units.contains(&unit_path))
            .unwrap_or(false);

        let action = if in_modified_unit {
            // Report the unit skip once per unit
            if let Some(unit_path) = find_unit_for_path(&dotfile.target_path, &units) {
                if !reported_units.contains(&unit_path) {
                    emit(
                        Level::Warn,
                        "dot.apply.skipped_unit",
                        &format!(
                            "{} Skipped unit ~/{} (contains modified files)",
                            char::from(NerdFont::ShieldAlert),
                            unit_path.display().to_string().yellow()
                        ),
                        Some(serde_json::json!({
                            "unit": format!("~/{}", unit_path.display()),
                            "action": "skipped_unit",
                            "reason": "unit_modified"
                        })),
                    );
                    reported_units.insert(unit_path);
                }
            }
            ApplyAction::SkippedUnit
        } else {
            apply_single_dotfile(dotfile, db)?
        };

        let relative_path = dotfile
            .target_path
            .strip_prefix(&home)
            .unwrap_or(&dotfile.target_path);

        match action {
            ApplyAction::Created => {
                let path_str = format!("~/{}", relative_path.display());
                emit(
                    Level::Success,
                    "dot.apply.created",
                    &format!(
                        "{} Created: {}",
                        char::from(NerdFont::Check),
                        path_str.green()
                    ),
                    Some(serde_json::json!({
                        "path": path_str,
                        "action": "created"
                    })),
                );
                created_files.push(path_str);
            }
            ApplyAction::Updated => {
                let path_str = format!("~/{}", relative_path.display());
                emit(
                    Level::Success,
                    "dot.apply.updated",
                    &format!(
                        "{} Updated: {}",
                        char::from(NerdFont::Check),
                        path_str.green()
                    ),
                    Some(serde_json::json!({
                        "path": path_str,
                        "action": "updated"
                    })),
                );
                updated_files.push(path_str);
            }
            ApplyAction::Skipped => {
                let path_str = format!("~/{}", relative_path.display());
                emit(
                    Level::Warn,
                    "dot.apply.skipped",
                    &format!(
                        "{} Skipped (user modified): {}",
                        char::from(NerdFont::ShieldAlert),
                        path_str.yellow()
                    ),
                    Some(serde_json::json!({
                        "path": path_str,
                        "action": "skipped",
                        "reason": "user_modified"
                    })),
                );
                skipped_files.push(path_str);
            }
            ApplyAction::SkippedUnit => {
                skipped_unit_count += 1;
            }
            ApplyAction::AlreadyUpToDate => {
                unchanged_count += 1;
            }
        }
    }

    db.cleanup_hashes(config.hash_cleanup_days)?;

    // Print summary
    print_apply_summary(
        created_files.len(),
        updated_files.len(),
        skipped_files.len(),
        skipped_unit_count,
        reported_units.len(),
        unchanged_count,
    );

    Ok(())
}

/// Apply a single dotfile and determine what action was taken
fn apply_single_dotfile(dotfile: &Dotfile, db: &Database) -> Result<ApplyAction> {
    let target_exists = dotfile.target_path.exists();
    let is_modified = !dotfile.is_target_unmodified(db)?;
    let is_outdated = dotfile.is_outdated(db);

    // Check if file is user-modified (would be skipped)
    if is_modified {
        return Ok(ApplyAction::Skipped);
    }

    // Check if file is already up-to-date
    if !is_outdated {
        let _ = dotfile.get_file_hash(&dotfile.source_path, true, db);
        return Ok(ApplyAction::AlreadyUpToDate);
    }

    // Apply the file
    dotfile.apply(db)?;

    // Determine if it was created or updated
    if !target_exists {
        Ok(ApplyAction::Created)
    } else {
        Ok(ApplyAction::Updated)
    }
}

/// Print summary of apply operation
fn print_apply_summary(
    created: usize,
    updated: usize,
    skipped: usize,
    skipped_unit_files: usize,
    skipped_units: usize,
    unchanged: usize,
) {
    separator(false);

    let summary_title = if matches!(get_output_format(), OutputFormat::Json) {
        "Apply Summary".to_string()
    } else {
        format!(
            "{} {} Apply Summary",
            char::from(NerdFont::Chart),
            char::from(NerdFont::List)
        )
    };

    let summary_data = serde_json::json!({
        "created": created,
        "updated": updated,
        "skipped": skipped,
        "skipped_unit_files": skipped_unit_files,
        "skipped_units": skipped_units,
        "unchanged": unchanged
    });

    if matches!(get_output_format(), OutputFormat::Json) {
        emit(Level::Info, "dot.apply.summary.title", &summary_title, None);
        let summary_text = format!(
            "  Created: {}\n  Updated: {}\n  Skipped: {}\n  Skipped (units): {} files in {} units\n  Unchanged: {}",
            created, updated, skipped, skipped_unit_files, skipped_units, unchanged
        );
        emit(
            Level::Info,
            "dot.apply.summary",
            &summary_text,
            Some(summary_data),
        );
        separator(false);
    } else {
        emit(
            Level::Info,
            "dot.apply.summary.title",
            &summary_title,
            Some(summary_data),
        );

        // Build entries dynamically - only show unit skips if there are any
        let mut entries: Vec<(Level, Option<char>, &str, String, &str)> = vec![
            (
                Level::Success,
                Some(char::from(NerdFont::Check)),
                "Created",
                created.to_string(),
                "dot.apply.summary.created",
            ),
            (
                Level::Success,
                Some(char::from(NerdFont::Check)),
                "Updated",
                updated.to_string(),
                "dot.apply.summary.updated",
            ),
        ];

        if skipped > 0 {
            entries.push((
                Level::Warn,
                Some(char::from(NerdFont::ShieldAlert)),
                "Skipped",
                skipped.to_string(),
                "dot.apply.summary.skipped",
            ));
        }

        if skipped_units > 0 {
            entries.push((
                Level::Warn,
                Some(char::from(NerdFont::ShieldAlert)),
                "Skipped Units",
                format!("{} files in {} units", skipped_unit_files, skipped_units),
                "dot.apply.summary.skipped_units",
            ));
        }

        entries.push((
            Level::Info,
            Some(char::from(NerdFont::Clock2)),
            "Unchanged",
            unchanged.to_string(),
            "dot.apply.summary.unchanged",
        ));

        let label_width = entries
            .iter()
            .map(|(_, _, label, _, _)| label.len())
            .max()
            .unwrap_or(0);
        let column_width = label_width + 4;

        for (level, icon, label, value, code) in entries {
            let label_with_icon = if matches!(get_output_format(), OutputFormat::Json) {
                format!("{label}:")
            } else {
                match icon {
                    Some(icon) => format!("{icon} {label}:"),
                    None => format!("  {label}:"),
                }
            };
            let padded_label = format!("{label_with_icon:<width$}", width = column_width);
            let message = format!("{padded_label} {value}");
            emit(
                level,
                code,
                &message,
                Some(serde_json::json!({
                    "label": label.to_lowercase(),
                    "count": value
                })),
            );
        }

        separator(false);
    }
}
