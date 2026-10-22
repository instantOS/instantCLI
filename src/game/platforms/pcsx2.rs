//! PCSX2 discovery settings. Ownership remains an explicit user choice.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};

use crate::game::ownership::normalized_path;

#[derive(Default)]
pub(crate) struct Settings {
    entries: BTreeMap<(String, String), Vec<String>>,
}

impl Settings {
    pub(crate) fn load(root: &Path) -> Result<Self> {
        let path = root.join("inis/PCSX2.ini");
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text).with_context(|| format!("Reading {}", path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).with_context(|| format!("Reading {}", path.display())),
        }
    }

    fn parse(text: &str) -> Result<Self> {
        let mut settings = Self::default();
        let mut section = String::new();
        for line in text.trim_start_matches('\u{feff}').lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with([';', '#']) {
                continue;
            }
            if let Some(name) = line
                .strip_prefix('[')
                .and_then(|line| line.strip_suffix(']'))
            {
                section = name.to_ascii_lowercase();
                continue;
            }
            if section != "folders" && section != "gamelist" {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim().to_ascii_lowercase();
            if !matches!(
                (section.as_str(), key.as_str()),
                ("folders", "memorycards" | "cache")
                    | ("gamelist", "paths" | "recursivepaths" | "excludedpaths")
            ) {
                continue;
            }
            let value = value.trim();
            ensure!(
                !value.starts_with('@') && !value.starts_with("<<<"),
                "Unsupported PCSX2 path encoding"
            );
            let value = if value.starts_with('"') {
                value
                    .strip_prefix('"')
                    .and_then(|value| value.strip_suffix('"'))
                    .context("Unterminated PCSX2 path")?
            } else {
                value
            };
            let entries = settings.entries.entry((section.clone(), key)).or_default();
            ensure!(
                section != "folders" || entries.is_empty(),
                "Duplicate PCSX2 folder setting"
            );
            if !value.is_empty() {
                entries.push(value.into());
            }
        }
        Ok(settings)
    }

    pub(crate) fn values(&self, section: &str, key: &str) -> &[String] {
        self.entries
            .get(&(section.to_ascii_lowercase(), key.to_ascii_lowercase()))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub(crate) fn folder(&self, root: &Path, name: &str, default: &str) -> Result<PathBuf> {
        let value = self
            .values("Folders", name)
            .first()
            .map(String::as_str)
            .unwrap_or(default);
        let path = Path::new(value);
        normalized_path(&if path.is_absolute() {
            path.into()
        } else {
            root.join(path)
        })
    }
}

pub fn configured_card_directory(root: &Path) -> Result<PathBuf> {
    Settings::load(root)?.folder(root, "MemoryCards", "memcards")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_repeated_game_paths_without_splitting_commas_or_spaces() -> Result<()> {
        let settings = Settings::parse(
            "[GameList]\nPaths = /games/one, two\nPaths = /games/three\nRecursivePaths = /games/deep\nExcludedPaths = /games/deep/skip\n",
        )?;
        assert_eq!(
            settings.values("GameList", "Paths"),
            ["/games/one, two", "/games/three"]
        );
        assert_eq!(
            settings.values("GameList", "ExcludedPaths"),
            ["/games/deep/skip"]
        );
        Ok(())
    }

    #[test]
    fn respects_folder_overrides_and_ignores_unrelated_encoding() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let settings = Settings::parse(
            "[Folders]\nMemoryCards = \"my cards\"\nCache = external-cache\nOther = @ByteArray(unrelated)\n[MemoryCards]\nSlot1_Filename = @ByteArray(unrelated)\n",
        )?;
        assert_eq!(
            settings.folder(temp.path(), "MemoryCards", "memcards")?,
            temp.path().join("my cards")
        );
        assert_eq!(
            settings.folder(temp.path(), "Cache", "cache")?,
            temp.path().join("external-cache")
        );
        assert!(Settings::parse("[Folders]\nCache = @ByteArray(unknown)\n").is_err());
        assert!(Settings::parse("[Folders]\nCache = one\nCache = two\n").is_err());
        Ok(())
    }
}
