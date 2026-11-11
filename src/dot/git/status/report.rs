use std::collections::HashMap;
use std::path::PathBuf;

use colored::Colorize;

use crate::dot::config::DotfileConfig;
use crate::dot::encryption::classify_encrypted_failure;
use crate::dot::git::{get_dotfile_dir_name, get_repo_name_for_dotfile};
use crate::dot::units::UnitIndex;

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

pub struct StatusReport {
    pub files_by_status: HashMap<DotFileStatus, Vec<FileInfo>>,
    pub summary: StatusSummary,
}

/// Categorize files by status and get summary statistics
pub fn collect_status(
    all_dotfiles: &HashMap<PathBuf, crate::dot::Dotfile>,
    cfg: &DotfileConfig,
    db: &crate::dot::db::Database,
    unit_index: &UnitIndex,
) -> StatusReport {
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
            repo_name,
            dotfile_dir,
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

    StatusReport {
        files_by_status,
        summary,
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
