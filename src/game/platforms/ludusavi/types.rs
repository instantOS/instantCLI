//! Data structures for Ludusavi manifest parsing

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use serde_yaml::Value;

/// Root manifest structure: game_name -> GameEntry
pub type LudusaviManifest = HashMap<String, GameEntry>;

/// A single game entry in the manifest
#[derive(Debug, Clone, Deserialize)]
pub struct GameEntry {
    #[serde(default)]
    pub files: HashMap<String, FileEntry>,
    #[serde(default)]
    #[allow(dead_code)]
    pub alias: Option<String>,
    #[serde(default, rename = "installDir")]
    pub install_dir: HashMap<String, Value>,
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
    #[allow(dead_code)]
    pub store: Option<String>,
}

/// A discovered save from a wine prefix scan
#[derive(Debug, Clone)]
pub struct DiscoveredWineSave {
    pub game_name: String,
    pub save_path: String,
    pub tags: Vec<String>,
    pub from_store_user_id: bool,
}

impl DiscoveredWineSave {
    pub fn new(
        game_name: String,
        save_path: String,
        tags: Vec<String>,
        from_store_user_id: bool,
    ) -> Self {
        Self {
            game_name,
            save_path,
            tags,
            from_store_user_id,
        }
    }

    /// Returns true if this entry has a 'save' tag
    #[allow(dead_code)]
    pub fn is_save(&self) -> bool {
        self.tags.iter().any(|t| t == "save")
    }

    /// Returns true if this entry has a 'config' tag
    #[allow(dead_code)]
    pub fn is_config(&self) -> bool {
        self.tags.iter().any(|t| t == "config")
    }

    fn store_user_id_match_quality(&self) -> u8 {
        if !self.from_store_user_id {
            return 0;
        }

        let Some(name) = Path::new(&self.save_path)
            .file_name()
            .and_then(|x| x.to_str())
        else {
            return 2;
        };

        let lower = name.to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "cache" | "caches" | "config" | "logs" | "preferences" | "settings" | "temp" | "tmp"
        ) {
            return 3;
        }

        if name.chars().all(|c| c.is_ascii_digit()) {
            return 0;
        }

        if name.len() >= 8 && name.chars().all(|c| c.is_ascii_hexdigit()) {
            return 0;
        }

        let hex_without_hyphens: String = name.chars().filter(|&c| c != '-').collect();
        if name.contains('-')
            && hex_without_hyphens.len() >= 8
            && hex_without_hyphens.chars().all(|c| c.is_ascii_hexdigit())
        {
            return 0;
        }

        1
    }
}

fn semantic_rank(save: &DiscoveredWineSave) -> (bool, bool, bool, u8, usize, usize) {
    let path = Path::new(&save.save_path);
    let is_dir = path.is_dir();
    let depth = path.components().count();

    (
        !is_dir,
        !save.is_save(),
        save.is_config(),
        save.store_user_id_match_quality(),
        depth,
        save.save_path.len(),
    )
}

fn entry_size(path: &Path) -> u64 {
    let Ok(metadata) = fs::metadata(path) else {
        return 0;
    };

    if metadata.is_file() {
        return metadata.len();
    }

    if !metadata.is_dir() {
        return 0;
    }

    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };

    entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry_size(&entry.path()))
        .sum()
}

pub fn choose_primary_save(mut saves: Vec<DiscoveredWineSave>) -> Option<DiscoveredWineSave> {
    saves.retain(DiscoveredWineSave::is_save);
    if saves.is_empty() {
        return None;
    }

    let mut size_cache = HashMap::new();
    saves.sort_by(|left, right| {
        let left_rank = semantic_rank(left);
        let right_rank = semantic_rank(right);

        left_rank.cmp(&right_rank).then_with(|| {
            let left_size = *size_cache
                .entry(left.save_path.clone())
                .or_insert_with(|| entry_size(Path::new(&left.save_path)));
            let right_size = *size_cache
                .entry(right.save_path.clone())
                .or_insert_with(|| entry_size(Path::new(&right.save_path)));

            right_size.cmp(&left_size)
        })
    });

    saves.into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choose_primary_save_prefers_directory_over_config_file() {
        let temp = tempfile::tempdir().unwrap();
        let save_dir = temp.path().join("Terraria");
        std::fs::create_dir_all(&save_dir).unwrap();
        let config_file = save_dir.join("config.json");
        std::fs::write(&config_file, "{}").unwrap();

        let selected = choose_primary_save(vec![
            DiscoveredWineSave::new(
                "Terraria".to_string(),
                config_file.display().to_string(),
                vec!["config".to_string()],
                false,
            ),
            DiscoveredWineSave::new(
                "Terraria".to_string(),
                save_dir.display().to_string(),
                vec!["save".to_string()],
                false,
            ),
        ])
        .unwrap();

        assert_eq!(selected.save_path, save_dir.display().to_string());
    }

    #[test]
    fn choose_primary_save_uses_size_only_for_semantic_ties() {
        let temp = tempfile::tempdir().unwrap();
        let game_root = temp.path().join("Terraria");
        let small_dir = game_root.join("slot-a");
        let large_dir = game_root.join("slot-b");
        std::fs::create_dir_all(&small_dir).unwrap();
        std::fs::create_dir_all(&large_dir).unwrap();
        std::fs::write(small_dir.join("save1.dat"), [0_u8; 4]).unwrap();
        std::fs::write(large_dir.join("save1.dat"), [0_u8; 64]).unwrap();

        let selected = choose_primary_save(vec![
            DiscoveredWineSave::new(
                "Terraria".to_string(),
                small_dir.display().to_string(),
                vec!["save".to_string()],
                false,
            ),
            DiscoveredWineSave::new(
                "Terraria".to_string(),
                large_dir.display().to_string(),
                vec!["save".to_string()],
                false,
            ),
        ])
        .unwrap();

        assert_eq!(selected.save_path, large_dir.display().to_string());
    }

    #[test]
    fn choose_primary_save_ignores_config_only_matches() {
        let temp = tempfile::tempdir().unwrap();
        let config_file = temp.path().join("ACU.ini");
        std::fs::write(&config_file, "{}").unwrap();

        let selected = choose_primary_save(vec![DiscoveredWineSave::new(
            "Assassin's Creed Unity".to_string(),
            config_file.display().to_string(),
            vec!["config".to_string()],
            false,
        )]);

        assert!(selected.is_none());
    }
}
