use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::path::Path;
use std::time::SystemTime;
use walkdir::WalkDir;

/// Information about a save directory's contents and timestamps
#[derive(Debug, Clone)]
pub struct SaveDirectoryInfo {
    pub last_modified: Option<SystemTime>,
    pub file_count: u64,
    pub total_size: u64,
}

/// Comparison result between snapshot time and local save time
#[derive(Debug, Clone, PartialEq)]
pub enum TimeComparison {
    LocalNewer,
    LocalNewerWithinTolerance(i64),
    SnapshotNewer,
    SnapshotNewerWithinTolerance(i64),
    Same,
    Error(String),
}

/// How close the comparison should treat timestamps as effectively identical (in seconds)
pub const SYNC_TOLERANCE_SECONDS: i64 = 300;

/// Get comprehensive information about a save directory
pub fn get_save_directory_info(save_path: &Path) -> Result<SaveDirectoryInfo> {
    if !save_path.exists() {
        return Ok(SaveDirectoryInfo {
            last_modified: None,
            file_count: 0,
            total_size: 0,
        });
    }

    let mut last_modified: Option<SystemTime> = None;
    let mut file_count = 0u64;
    let mut total_size = 0u64;

    // Walk through the directory to find information
    for entry in WalkDir::new(save_path)
        .into_iter()
        .filter_entry(|e| !is_hidden(e))
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            file_count += 1;

            // Get file metadata for size and modification time
            if let Ok(metadata) = entry.metadata() {
                total_size += metadata.len();

                if let Ok(modified_time) = metadata.modified() {
                    // Update the most recent modification time
                    if last_modified.is_none_or(|current| modified_time > current) {
                        last_modified = Some(modified_time);
                    }
                }
            }
        }
    }

    Ok(SaveDirectoryInfo {
        last_modified,
        file_count,
        total_size,
    })
}

/// Get only the most recent file modification time in a directory
pub fn get_most_recent_file_time(save_path: &Path) -> Result<Option<SystemTime>> {
    let info = get_save_directory_info(save_path)?;
    Ok(info.last_modified)
}

/// Parse snapshot time string (ISO 8601) to DateTime<Utc>
pub fn parse_snapshot_time(iso_time: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(iso_time)
        .map(|dt| dt.with_timezone(&Utc))
        .context("Failed to parse snapshot time")
}

/// Convert SystemTime to DateTime<Utc>
pub fn system_time_to_datetime(system_time: SystemTime) -> DateTime<Utc> {
    DateTime::<Utc>::from(system_time)
}

/// Format SystemTime for display (human-readable)
pub fn format_system_time_for_display(system_time: Option<SystemTime>) -> String {
    match system_time {
        Some(time) => {
            let datetime = DateTime::<Utc>::from(time);
            let now = Utc::now();
            let duration = now.signed_duration_since(datetime);

            // Format based on how recent the modification was
            if duration.num_seconds() < 60 {
                format!("{} seconds ago", duration.num_seconds().abs())
            } else if duration.num_minutes() < 60 {
                format!("{} minutes ago", duration.num_minutes().abs())
            } else if duration.num_hours() < 24 {
                format!("{} hours ago", duration.num_hours().abs())
            } else if duration.num_days() < 7 {
                format!("{} days ago", duration.num_days().abs())
            } else {
                datetime.format("%Y-%m-%d").to_string()
            }
        }
        None => "Never".to_string(),
    }
}

/// Compare snapshot time with local save directory time
///
/// If local saves are within 5 minutes of the snapshot, considers them the same
/// (losing 5 minutes of progress is acceptable)
pub fn compare_snapshot_vs_local(
    snapshot_time_str: &str,
    local_time: SystemTime,
) -> Result<TimeComparison> {
    let snapshot_dt = parse_snapshot_time(snapshot_time_str)?;
    let local_dt = system_time_to_datetime(local_time);

    if local_dt == snapshot_dt {
        return Ok(TimeComparison::Same);
    }

    // Calculate the absolute time difference
    let duration = if local_dt > snapshot_dt {
        local_dt.signed_duration_since(snapshot_dt)
    } else {
        snapshot_dt.signed_duration_since(local_dt)
    };

    let delta_seconds = duration.num_seconds().abs();

    if delta_seconds <= SYNC_TOLERANCE_SECONDS {
        if local_dt > snapshot_dt {
            return Ok(TimeComparison::LocalNewerWithinTolerance(delta_seconds));
        } else {
            return Ok(TimeComparison::SnapshotNewerWithinTolerance(delta_seconds));
        }
    }

    if local_dt > snapshot_dt {
        Ok(TimeComparison::LocalNewer)
    } else {
        Ok(TimeComparison::SnapshotNewer)
    }
}

/// Format file size in human-readable format
pub fn format_file_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];

    if bytes == 0 {
        return "0 B".to_string();
    }

    let bytes = bytes as f64;
    let base = 1024_f64;
    let i = (bytes.ln() / base.ln()).floor() as usize;
    let i = i.min(UNITS.len() - 1);

    format!("{:.1} {}", bytes / base.powi(i as i32), UNITS[i])
}

/// Helper function to check if a directory entry is hidden
fn is_hidden(entry: &walkdir::DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .map(|s| s.starts_with('.'))
        .unwrap_or(false)
}
