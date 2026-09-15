//! Persistence for the record of the most recent log upload.
//!
//! The record is deliberately kept outside [`super::state::InstallState`]:
//! install-resume progress and upload history have different lifetimes. A
//! record only stays meaningful while the install log it was generated from
//! is unchanged, so it is keyed to the log's contents and reported as absent
//! once a new install attempt rewrites the log.

use std::fs;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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
/// upload. Tied to the install log's SHA-256 digest at upload time: once the
/// log changes, the record no longer describes it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UploadRecord {
    pub url: String,
    pub scope: UploadScope,
    pub uploaded_at: chrono::DateTime<chrono::Utc>,
    /// SHA-256 digest of the exact install log the report was built from.
    pub log_sha256: String,
}

impl UploadRecord {
    pub fn new(url: impl Into<String>, scope: UploadScope, log_sha256: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            scope,
            uploaded_at: chrono::Utc::now(),
            log_sha256: log_sha256.into(),
        }
    }

    /// Load the remembered upload without deciding whether it is still current.
    pub fn load() -> Option<Self> {
        let content = match fs::read_to_string(paths::UPLOAD_STATE_FILE) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
            Err(error) => {
                eprintln!("Warning: could not read the upload record: {error}");
                return None;
            }
        };
        match toml::from_str(&content) {
            Ok(record) => record,
            Err(error) => {
                eprintln!("Warning: could not parse the upload record: {error}");
                None
            }
        }
    }

    /// The remembered upload, if it still describes the current install log.
    pub fn current() -> Option<Self> {
        let record = Self::load()?;
        let log_sha256 = fingerprint_file(Path::new(paths::LOG_FILE)).ok()?;
        (record.log_sha256 == log_sha256).then_some(record)
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

/// Produce a content identity for a log file.
pub fn fingerprint_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| {
        format!(
            "Failed to read log file for fingerprinting: {}",
            path.display()
        )
    })?;
    Ok(fingerprint_bytes(&bytes))
}

pub fn fingerprint_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_record_round_trips_through_toml_with_enum_scope() {
        let record = UploadRecord::new(
            "https://snips.sh/f/abc",
            UploadScope::InstallLog,
            fingerprint_bytes(b"install log"),
        );

        let encoded = toml::to_string_pretty(&record).unwrap();
        assert!(encoded.contains("scope = \"InstallLog\""));

        let decoded: UploadRecord = toml::from_str(&encoded).unwrap();
        assert_eq!(decoded, record);
    }

    #[test]
    fn fingerprint_changes_with_log_contents() {
        assert_ne!(fingerprint_bytes(b"first"), fingerprint_bytes(b"second"));
    }
}
