use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::dotfile::Dotfile;
use crate::dot::units::{get_all_units, get_modified_units};
use crate::dot::utils::get_all_dotfiles;
use crate::ui::prelude::*;
use anyhow::Result;
use colored::Colorize;
use std::collections::HashSet;
use std::path::PathBuf;

/// Result of applying a single dotfile
#[derive(Debug, PartialEq)]
enum ApplyAction {
    Created,
    Updated,
    Skipped,
    SkippedUnit,
    AlreadyUpToDate,
}

/// Statistics collected during apply operation
#[derive(Default)]
struct ApplyStats {
    created: Vec<String>,
    updated: Vec<String>,
    skipped: Vec<String>,
    skipped_unit_files: usize,
    unchanged: usize,
    reported_units: HashSet<PathBuf>,
}

impl ApplyStats {
    fn skipped_units(&self) -> usize {
        self.reported_units.len()
    }
}

/// Apply all dotfiles from configured repositories
pub fn apply_all(config: &Config, db: &Database) -> Result<()> {
    let all_dotfiles = get_all_dotfiles(config, db)?;

    if all_dotfiles.is_empty() {
        emit(
            Level::Info,
            "dot.apply.no_dotfiles",
            &format!("{} No dotfiles configured", char::from(NerdFont::Info)),
            None,
        );
        return Ok(());
    }

    // Get unit definitions and find which units have modified files
    let units = get_all_units(config, db)?;
    let modified_units = get_modified_units(&all_dotfiles, &units, db)?;

    let mut stats = ApplyStats::default();

    // Apply each dotfile
    for dotfile in all_dotfiles.values() {
        let action = determine_and_apply_action(dotfile, &units, &modified_units, &mut stats, db)?;
        emit_action_result(&action, dotfile);
        record_action(&action, dotfile, &mut stats);
    }

    db.cleanup_hashes(config.hash_cleanup_days)?;
    print_apply_summary(&stats);

    Ok(())
}

/// Determine and execute the appropriate action for a dotfile
fn determine_and_apply_action(
    dotfile: &Dotfile,
    units: &[PathBuf],
    modified_units: &HashSet<PathBuf>,
    stats: &mut ApplyStats,
    db: &Database,
) -> Result<ApplyAction> {
    // Check if file belongs to a modified unit
    let unit_paths = crate::dot::units::find_units_for_path(&dotfile.target_path, units);
    if !unit_paths.is_empty() {
        let mut should_skip = false;
        for unit_path in unit_paths {
            if !modified_units.contains(&unit_path) {
                continue;
            }

            should_skip = true;

            // Report the unit skip once per unit
            if !stats.reported_units.contains(&unit_path) {
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
                stats.reported_units.insert(unit_path);
            }
        }

        if should_skip {
            return Ok(ApplyAction::SkippedUnit);
        }
    }

    apply_single_dotfile(dotfile, db)
}

/// Apply a single dotfile and determine what action was taken
fn apply_single_dotfile(dotfile: &Dotfile, db: &Database) -> Result<ApplyAction> {
    let target_exists = dotfile.target_path.exists();
    let is_modified = !dotfile.is_target_unmodified(db)?;
    let is_outdated = dotfile.is_outdated(db);

    if is_modified {
        return Ok(ApplyAction::Skipped);
    }

    if !is_outdated {
        let _ = dotfile.get_file_hash(&dotfile.source_path, true, db);
        return Ok(ApplyAction::AlreadyUpToDate);
    }

    dotfile.apply(db)?;

    if !target_exists {
        Ok(ApplyAction::Created)
    } else {
        Ok(ApplyAction::Updated)
    }
}

/// Emit user-visible output for an action
fn emit_action_result(action: &ApplyAction, dotfile: &Dotfile) {
    let home = PathBuf::from(shellexpand::tilde("~").to_string());
    let relative_path = dotfile
        .target_path
        .strip_prefix(&home)
        .unwrap_or(&dotfile.target_path);
    let path_str = format!("~/{}", relative_path.display());

    match action {
        ApplyAction::Created => {
            emit(
                Level::Success,
                "dot.apply.created",
                &format!(
                    "{} Created: {}",
                    char::from(NerdFont::Check),
                    path_str.green()
                ),
                Some(serde_json::json!({"path": path_str, "action": "created"})),
            );
        }
        ApplyAction::Updated => {
            emit(
                Level::Success,
                "dot.apply.updated",
                &format!(
                    "{} Updated: {}",
                    char::from(NerdFont::Check),
                    path_str.green()
                ),
                Some(serde_json::json!({"path": path_str, "action": "updated"})),
            );
        }
        ApplyAction::Skipped => {
            emit(
                Level::Warn,
                "dot.apply.skipped",
                &format!(
                    "{} Skipped (user modified): {}",
                    char::from(NerdFont::ShieldAlert),
                    path_str.yellow()
                ),
                Some(
                    serde_json::json!({"path": path_str, "action": "skipped", "reason": "user_modified"}),
                ),
            );
        }
        ApplyAction::SkippedUnit | ApplyAction::AlreadyUpToDate => {}
    }
}

/// Record action results into stats
fn record_action(action: &ApplyAction, dotfile: &Dotfile, stats: &mut ApplyStats) {
    let home = PathBuf::from(shellexpand::tilde("~").to_string());
    let relative_path = dotfile
        .target_path
        .strip_prefix(&home)
        .unwrap_or(&dotfile.target_path);
    let path_str = format!("~/{}", relative_path.display());

    match action {
        ApplyAction::Created => stats.created.push(path_str),
        ApplyAction::Updated => stats.updated.push(path_str),
        ApplyAction::Skipped => stats.skipped.push(path_str),
        ApplyAction::SkippedUnit => stats.skipped_unit_files += 1,
        ApplyAction::AlreadyUpToDate => stats.unchanged += 1,
    }
}

/// Print summary of apply operation
fn print_apply_summary(stats: &ApplyStats) {
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
        "created": stats.created.len(),
        "updated": stats.updated.len(),
        "skipped": stats.skipped.len(),
        "skipped_unit_files": stats.skipped_unit_files,
        "skipped_units": stats.skipped_units(),
        "unchanged": stats.unchanged
    });

    if matches!(get_output_format(), OutputFormat::Json) {
        emit(Level::Info, "dot.apply.summary.title", &summary_title, None);
        emit(
            Level::Info,
            "dot.apply.summary",
            &format!(
                "  Created: {}\n  Updated: {}\n  Skipped: {}\n  Skipped (units): {} files in {} units\n  Unchanged: {}",
                stats.created.len(),
                stats.updated.len(),
                stats.skipped.len(),
                stats.skipped_unit_files,
                stats.skipped_units(),
                stats.unchanged
            ),
            Some(summary_data),
        );
        separator(false);
        return;
    }

    emit(
        Level::Info,
        "dot.apply.summary.title",
        &summary_title,
        Some(summary_data.clone()),
    );

    let mut entries: Vec<(Level, char, &str, String, &str)> = vec![
        (
            Level::Success,
            char::from(NerdFont::Check),
            "Created",
            stats.created.len().to_string(),
            "dot.apply.summary.created",
        ),
        (
            Level::Success,
            char::from(NerdFont::Check),
            "Updated",
            stats.updated.len().to_string(),
            "dot.apply.summary.updated",
        ),
    ];

    if !stats.skipped.is_empty() {
        entries.push((
            Level::Warn,
            char::from(NerdFont::ShieldAlert),
            "Skipped",
            stats.skipped.len().to_string(),
            "dot.apply.summary.skipped",
        ));
    }

    if stats.skipped_units() > 0 {
        entries.push((
            Level::Warn,
            char::from(NerdFont::ShieldAlert),
            "Skipped Units",
            format!(
                "{} files in {} units",
                stats.skipped_unit_files,
                stats.skipped_units()
            ),
            "dot.apply.summary.skipped_units",
        ));
    }

    entries.push((
        Level::Info,
        char::from(NerdFont::Clock2),
        "Unchanged",
        stats.unchanged.to_string(),
        "dot.apply.summary.unchanged",
    ));

    let label_width = entries
        .iter()
        .map(|(_, _, l, _, _)| l.len())
        .max()
        .unwrap_or(0)
        + 4;

    for (level, icon, label, value, code) in entries {
        let label_with_icon = format!("{icon} {label}:");
        let message = format!("{label_with_icon:<width$} {value}", width = label_width);
        emit(
            level,
            code,
            &message,
            Some(serde_json::json!({"label": label.to_lowercase(), "count": value})),
        );
    }

    separator(false);
}
