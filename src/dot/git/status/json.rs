use crate::dot::units::UnitIndex;

use super::report::{FileInfo, StatusReport};
use super::{DotFileStatus, RenderContext};

/// Show status in JSON format
pub(super) fn document(
    report: &StatusReport,
    show_all: bool,
    show_sources: bool,
    context: &RenderContext<'_>,
    unit_index: &UnitIndex,
) -> serde_json::Value {
    let files_by_status = &report.files_by_status;
    let summary = &report.summary;
    let files = |status| -> Vec<serde_json::Value> {
        if status == DotFileStatus::Clean && !show_all {
            return Vec::new();
        }
        files_by_status
            .get(&status)
            .into_iter()
            .flatten()
            .map(|file| file_json(file, status, context, show_sources))
            .collect()
    };

    let modified_units: Vec<_> = unit_index
        .modified_unit_statuses()
        .into_iter()
        .map(|unit_status| {
            let modified_files: Vec<_> = unit_status
                .modified_files
                .iter()
                .map(|path| context.path(path))
                .collect();
            serde_json::json!({
                "path": format!("~/{}", unit_status.unit_path.display()),
                "status": "modified",
                "modified_files": modified_files
            })
        })
        .collect();

    serde_json::json!({
        "total_files": summary.total_files,
        "clean_count": summary.clean_count,
        "modified_count": summary.modified_count,
        "modified_unit_count": modified_units.len(),
        "outdated_count": summary.outdated_count,
        "identity_required_count": summary.identity_required_count,
        "encrypted_error_count": summary.encrypted_error_count,
        "modified_files": files(DotFileStatus::Modified),
        "modified_units": modified_units,
        "outdated_files": files(DotFileStatus::Outdated),
        "identity_required_files": files(DotFileStatus::IdentityRequired),
        "encrypted_error_files": files(DotFileStatus::EncryptedError),
        "clean_files": files(DotFileStatus::Clean),
        "show_all": show_all,
        "show_sources": show_sources
    })
}

fn file_json(
    file: &FileInfo,
    status: DotFileStatus,
    context: &RenderContext<'_>,
    show_sources: bool,
) -> serde_json::Value {
    let (name, reason) = match status {
        DotFileStatus::Modified => ("modified", None),
        DotFileStatus::Outdated => ("outdated", None),
        DotFileStatus::Clean => ("clean", None),
        DotFileStatus::IdentityRequired => ("identity_required", Some("encrypted_source")),
        DotFileStatus::EncryptedError => ("encrypted_error", Some("encrypted_source_error")),
    };
    let mut value = serde_json::json!({
        "path": context.path(&file.target_path),
        "status": name,
        "repo": file.repo_name.as_str(),
        "dotfile_dir": file.dotfile_dir,
    });
    if let Some(reason) = reason {
        value["reason"] = reason.into();
    }
    if show_sources {
        value["priority"] = context.priority(file.repo_name.as_str()).into();
        value["override"] = file.is_overridden.into();
    }
    value
}
