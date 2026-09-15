//! Persistence for the record of the most recent log upload.
//!
//! The record is deliberately kept outside [`super::state::InstallState`]:
//! install-resume progress and upload history have different lifetimes. A
//! record only stays meaningful while the install log it was generated from
//! is unchanged, so it is keyed to the log's modification time and reported
//! as absent once a new install attempt rewrites the log.

use std::fs;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use super::paths;

/// What a support report includes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UploadScope {
    InstallLog,
    InstallLogAndSystemDetails,
}

impl UploadScope {
    /// Human-readable description of what the uploaded report contains.
    pub fn label(self) -> &'static str {
        match self {
            Self::InstallLog => "Install log and anonymous choices",
            Self::InstallLogAndSystemDetails => "Install log and system details",
        }
    }
}

/// The most recent successful upload of the installation report.
///
/// Kept so the menus can report that logs were already uploaded, show the
/// resulting URL again, and avoid surprising the user with a duplicate
/// upload. Tied to the install log's modification time at upload time: once
/// the log changes, the record no longer describes it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UploadRecord {
    pub url: String,
    pub scope: UploadScope,
    pub uploaded_at: chrono::DateTime<chrono::Utc>,
    /// Modification time of the install log the report was built from.
    pub log_modified: chrono::DateTime<chrono::Utc>,
}

impl UploadRecord {
    pub fn new(
        url: impl Into<String>,
        scope: UploadScope,
        log_modified: chrono::DateTime<chrono::Utc>,
    ) -> Self {
        Self {
            url: url.into(),
            scope,
            uploaded_at: chrono::Utc::now(),
            log_modified: truncate_seconds(log_modified),
        }
    }

    /// The remembered upload, if it still describes the current install log.
    pub fn current() -> Option<Self> {
        let content = match fs::read_to_string(paths::UPLOAD_STATE_FILE) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
            Err(error) => {
                eprintln!("Warning: could not read the upload record: {error}");
                return None;
            }
        };
        let record: Self = match toml::from_str(&content) {
            Ok(record) => record,
            Err(error) => {
                eprintln!("Warning: could not parse the upload record: {error}");
                return None;
            }
        };
        let log_modified = Self::current_log_modified()?;
        (record.log_modified == log_modified).then_some(record)
    }

    /// Modification time of the install log, truncated to whole seconds so
    /// records compare stably across serialization.
    pub fn current_log_modified() -> Option<chrono::DateTime<chrono::Utc>> {
        let modified = fs::metadata(paths::LOG_FILE).ok()?.modified().ok()?;
        Some(truncate_seconds(modified.into()))
    }

    /// Persist this record, replacing any previous one.
    ///
    /// The replacement is atomic so a crash cannot leave a half-written
    /// record behind.
    pub fn save(&self) -> Result<()> {
        let path = Path::new(paths::UPLOAD_STATE_FILE);
        let parent = path.parent().context("upload state path has no parent")?;
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
        let mut temp = NamedTempFile::new_in(parent)?;
        temp.write_all(toml::to_string_pretty(self)?.as_bytes())?;
        temp.persist(path)
            .context("Failed to persist upload record")?;
        Ok(())
    }
}

fn truncate_seconds(time: chrono::DateTime<chrono::Utc>) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(time.timestamp(), 0)
        .expect("whole-second timestamp is representable")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_record_round_trips_through_toml_with_enum_scope() {
        let log_modified = chrono::DateTime::from_timestamp(1_000_000, 0).unwrap();
        let record = UploadRecord::new(
            "https://snips.sh/f/abc",
            UploadScope::InstallLog,
            log_modified,
        );

        let encoded = toml::to_string_pretty(&record).unwrap();
        assert!(encoded.contains("scope = \"InstallLog\""));

        let decoded: UploadRecord = toml::from_str(&encoded).unwrap();
        assert_eq!(decoded, record);
    }

    #[test]
    fn log_time_is_truncated_to_whole_seconds() {
        let with_nanos = chrono::DateTime::from_timestamp(1_000_000, 999_999_999).unwrap();
        let record = UploadRecord::new(
            "https://snips.sh/f/abc",
            UploadScope::InstallLog,
            with_nanos,
        );

        assert_eq!(record.log_modified.timestamp(), 1_000_000);
        assert_eq!(record.log_modified.timestamp_subsec_nanos(), 0);
    }
}
