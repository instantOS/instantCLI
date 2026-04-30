use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::common::config::DocumentedConfig;
use crate::common::paths;
use crate::documented_config;

/// A single directory that resolvething scans for duplicates and Syncthing
/// conflict files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanDir {
    /// Directory to scan.
    pub path: PathBuf,
    /// File extensions (without the leading dot) treated as mergeable
    /// Syncthing conflicts inside this directory. An empty list means "all
    /// plain text files" (detected heuristically by size and absence of NUL
    /// bytes).
    #[serde(default)]
    pub extensions: Vec<String>,
}

impl ScanDir {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            extensions: Vec::new(),
        }
    }

    pub fn normalized_extensions(&self) -> Vec<String> {
        normalize_extensions(&self.extensions)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ResolvethingConfig {
    pub scan_dirs: Vec<ScanDir>,
    pub editor_command: Option<String>,
}

impl Default for ResolvethingConfig {
    fn default() -> Self {
        Self {
            scan_dirs: vec![ScanDir {
                path: default_scan_path(),
                extensions: vec!["md".to_string(), "json".to_string()],
            }],
            editor_command: None,
        }
    }
}

documented_config!(
    ResolvethingConfig,
    scan_dirs, "List of directories to scan; each entry has a path and a list of file extensions (empty = all plain text files)",
    editor_command, "Optional editor command used for conflict diffs; defaults to $EDITOR or nvim",
    => paths::instant_config_dir()?.join("resolvething.toml")
);

impl ResolvethingConfig {
    pub fn load() -> Result<Self> {
        <Self as DocumentedConfig>::load_from_path_documented(Self::config_path()?)
            .context("loading resolvething config")
    }

    pub fn save(&self) -> Result<()> {
        self.save_with_documentation(&Self::config_path()?)
            .context("saving resolvething config")
    }

    pub fn config_path() -> Result<PathBuf> {
        Ok(paths::instant_config_dir()?.join("resolvething.toml"))
    }

    /// Resolve a scan directory entry, expanding tildes and environment
    /// variables in the configured path.
    pub fn resolved_scan_dir(&self, index: usize) -> Result<ResolvedScanDir> {
        let entry = self
            .scan_dirs
            .get(index)
            .with_context(|| format!("scan_dir index {} out of range", index))?;
        Ok(ResolvedScanDir {
            path: expand_path(&entry.path.to_string_lossy())?,
            extensions: entry.normalized_extensions(),
            source_index: Some(index),
        })
    }

    /// Resolve all configured scan directories.
    pub fn resolved_scan_dirs(&self) -> Result<Vec<ResolvedScanDir>> {
        (0..self.scan_dirs.len())
            .map(|index| self.resolved_scan_dir(index))
            .collect()
    }

    /// Build an ad-hoc resolved scan dir from a CLI override path. Uses the
    /// extensions from the matching configured entry if the path matches one
    /// exactly; otherwise extensions default to empty (all text files).
    pub fn resolved_scan_dir_for_override(&self, raw: &str) -> Result<ResolvedScanDir> {
        let path = expand_path(raw)?;
        if let Some((index, entry)) =
            self.scan_dirs.iter().enumerate().find(|(_, e)| {
                expand_path(&e.path.to_string_lossy()).ok().as_deref() == Some(&path)
            })
        {
            return Ok(ResolvedScanDir {
                path,
                extensions: entry.normalized_extensions(),
                source_index: Some(index),
            });
        }
        Ok(ResolvedScanDir {
            path,
            extensions: Vec::new(),
            source_index: None,
        })
    }
}

/// A scan directory after path expansion and extension normalization.
#[derive(Debug, Clone)]
pub struct ResolvedScanDir {
    pub path: PathBuf,
    pub extensions: Vec<String>,
    /// Index into `ResolvethingConfig.scan_dirs` if this entry came from
    /// configuration; `None` for ad-hoc overrides.
    #[allow(dead_code)]
    pub source_index: Option<usize>,
}

impl ResolvedScanDir {
    pub fn display_path(&self) -> String {
        format_path(&self.path)
    }

    pub fn extensions_label(&self) -> String {
        if self.extensions.is_empty() {
            "all text files".to_string()
        } else {
            self.extensions.join(", ")
        }
    }
}

pub fn normalize_extensions(extensions: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for ty in extensions {
        let normalized = ty.trim().trim_start_matches('.').to_ascii_lowercase();
        if !normalized.is_empty() && !out.contains(&normalized) {
            out.push(normalized);
        }
    }
    out
}

pub fn expand_path(raw: &str) -> Result<PathBuf> {
    let expanded = shellexpand::full(raw)
        .with_context(|| format!("expanding path '{}'", raw))?
        .into_owned();
    Ok(PathBuf::from(expanded))
}

pub fn format_path(path: &Path) -> String {
    crate::common::TildePath::new(path.to_path_buf()).display_string()
}

fn default_scan_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("wiki")
        .join("vimwiki")
}
