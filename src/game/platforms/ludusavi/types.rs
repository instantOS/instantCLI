//! Data structures for Ludusavi manifest parsing

use std::collections::HashMap;

use serde::Deserialize;

/// Root manifest structure: game_name -> GameEntry
pub type LudusaviManifest = HashMap<String, GameEntry>;

/// A single game entry in the manifest
#[derive(Debug, Clone, Deserialize)]
pub struct GameEntry {
    #[serde(default)]
    pub files: HashMap<String, FileEntry>,
    #[serde(default)]
    pub alias: Option<String>,
}

/// A file/directory entry with constraints and tags
#[derive(Debug, Clone, Deserialize)]
pub struct FileEntry {
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub when: Vec<FileConstraint>,
}

/// OS/Store constraint for a file entry
#[derive(Debug, Clone, Deserialize)]
pub struct FileConstraint {
    #[serde(default)]
    pub os: Option<String>,
    #[serde(default)]
    pub store: Option<String>,
}

/// A discovered save from a wine prefix scan
#[derive(Debug, Clone)]
pub struct DiscoveredWineSave {
    pub game_name: String,
    pub save_path: String,
    pub tags: Vec<String>,
}

impl DiscoveredWineSave {
    pub fn new(game_name: String, save_path: String, tags: Vec<String>) -> Self {
        Self {
            game_name,
            save_path,
            tags,
        }
    }

    /// Returns true if this entry has a 'save' tag
    pub fn is_save(&self) -> bool {
        self.tags.iter().any(|t| t == "save")
    }

    /// Returns true if this entry has a 'config' tag
    pub fn is_config(&self) -> bool {
        self.tags.iter().any(|t| t == "config")
    }
}
