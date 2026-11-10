use colored::{Color, Colorize};

use crate::dot::units::UnitIndex;
use crate::ui::nerd_font::NerdFont;

use super::report::{FileInfo, StatusReport};
use super::{DotFileStatus, RenderContext};

/// Show status in text format
pub(super) fn render(
    report: &StatusReport,
    show_all: bool,
    show_sources: bool,
    context: &RenderContext<'_>,
    unit_index: &UnitIndex,
) {
    let files_by_status = &report.files_by_status;
    let summary = &report.summary;
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

    if summary.modified_count > 0
        || !modified_units.is_empty()
        || summary.outdated_count > 0
        || summary.identity_required_count > 0
        || summary.encrypted_error_count > 0
    {
        println!();
        if !modified_units.is_empty() {
            show_modified_units(&modified_units, context);
        }
    }
    for status in [
        DotFileStatus::Modified,
        DotFileStatus::Outdated,
        DotFileStatus::IdentityRequired,
        DotFileStatus::EncryptedError,
        DotFileStatus::Clean,
    ] {
        if status == DotFileStatus::Clean && !show_all {
            continue;
        }
        let files: Vec<_> = files_by_status
            .get(&status)
            .into_iter()
            .flatten()
            .filter(|file| {
                status != DotFileStatus::Modified
                    || !unit_index.is_target_in_modified_unit(&file.target_path)
            })
            .collect();
        if !files.is_empty() {
            show_files(&files, status, context, show_sources);
        }
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

fn show_files(
    files: &[&FileInfo],
    status: DotFileStatus,
    context: &RenderContext<'_>,
    show_sources: bool,
) {
    let (heading, label, color) = match status {
        DotFileStatus::Modified => (" Standalone modified files:", "modified", Color::Yellow),
        DotFileStatus::Outdated => ("Outdated files:", "outdated", Color::Blue),
        DotFileStatus::IdentityRequired => (
            "Encrypted files needing identity:",
            "identity required",
            Color::Yellow,
        ),
        DotFileStatus::EncryptedError => (
            "Encrypted files with errors:",
            "encrypted error",
            Color::Red,
        ),
        DotFileStatus::Clean => ("Clean files:", "clean", Color::Green),
    };
    println!("{}", heading.color(color).bold());
    for file in files {
        let path = context.path(&file.target_path);
        let override_indicator = if file.is_overridden {
            " [override]"
        } else {
            ""
        };
        if show_sources {
            println!(
                "  {} → {} / {} (P{}){}",
                path,
                file.repo_name.as_str().bright_purple(),
                file.dotfile_dir,
                context.priority(file.repo_name.as_str()),
                override_indicator.magenta()
            );
        } else {
            println!(
                "  {} -> {} ({}: {}{})",
                path,
                label.color(color),
                file.repo_name,
                file.dotfile_dir,
                override_indicator.magenta()
            );
        }
    }
    println!();
}

fn show_modified_units(
    modified_units: &[crate::dot::units::UnitStatus],
    context: &RenderContext<'_>,
) {
    println!("{}", " Modified units:".yellow().bold());
    for unit_status in modified_units {
        println!(
            "  {}",
            format!("~/{}", unit_status.unit_path.display()).yellow()
        );
        println!("    Modified files causing this unit state:");
        for path in &unit_status.modified_files {
            println!("      - {}", context.path(path));
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
            println!("  Use '{bin} dot reset <path>' to restore files to their original state");
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
        println!("  Use '{bin} dot status --all' to see all tracked files including clean ones");
    } else if clean_count > 0 {
        println!(
            "{} All dotfiles are clean and up to date!",
            char::from(NerdFont::Check).to_string().green()
        );
    } else {
        println!("No dotfiles found. Use '{bin} dot repo clone <url>' to clone a repository.");
    }
}
