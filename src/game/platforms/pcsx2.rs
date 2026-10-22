//! Locate PCSX2 memory cards for discovery. Save ownership is explicitly
//! configured by the user; launch does not infer it from emulator settings.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};

use crate::game::ownership::normalized_path;

type Settings = BTreeMap<(String, String), String>;

fn read_settings(path: &Path) -> Result<Settings> {
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("Reading PCSX2 configuration {}", path.display()))?;
    let mut result = Settings::new();
    let mut section = String::new();
    for line in contents.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].into();
            continue;
        }
        if section != "Folders" {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .context("Unrecognized PCSX2 save setting; refusing to guess")?;
        if key.trim() != "MemoryCards" {
            continue;
        }
        let value = value.trim();
        ensure!(
            !value.contains('\\') && !value.starts_with('@'),
            "Unsupported PCSX2 setting encoding in {}; inspect the save mapping manually",
            path.display()
        );
        let value = if value.starts_with('"') {
            value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .context("Unterminated PCSX2 setting")?
        } else {
            value
        };
        ensure!(
            result
                .insert((section.clone(), key.trim().into()), value.into())
                .is_none(),
            "Duplicate PCSX2 save setting in {}",
            path.display()
        );
    }
    Ok(result)
}

fn setting<'a>(settings: &'a Settings, section: &str, key: &str) -> Option<&'a str> {
    settings
        .get(&(section.into(), key.into()))
        .map(String::as_str)
}
fn folder(settings: &Settings, root: &Path, name: &str, default: &str) -> PathBuf {
    let value = Path::new(setting(settings, "Folders", name).unwrap_or(default));
    if value.is_absolute() {
        value.into()
    } else {
        root.join(value)
    }
}

pub fn configured_card_directory(root: &Path) -> Result<PathBuf> {
    let path = root.join("inis/PCSX2.ini");
    if !path.exists() {
        return normalized_path(&root.join("memcards"));
    }
    let settings = read_settings(&path)?;
    normalized_path(&folder(&settings, root, "MemoryCards", "memcards"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_ignores_unrelated_card_encodings_and_respects_card_directory() -> Result<()> {
        let temp = tempfile::tempdir()?;
        std::fs::create_dir(temp.path().join("inis"))?;
        std::fs::write(
            temp.path().join("inis/PCSX2.ini"),
            "[Folders]\nMemoryCards = custom-cards\nOther = @ByteArray(unrelated)\n[MemoryCards]\nSlot1_Filename = @ByteArray(unrelated)\n",
        )?;
        assert_eq!(
            configured_card_directory(temp.path())?,
            temp.path().join("custom-cards")
        );
        Ok(())
    }

    #[test]
    fn parser_handles_relevant_quoted_values_and_rejects_ambiguous_encoding() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("PCSX2.ini");
        std::fs::write(
            &path,
            "[Folders]\nMemoryCards = \"my cards\"\n[MemoryCards]\nSlot1_Enable = true\n",
        )?;
        assert_eq!(
            setting(&read_settings(&path)?, "Folders", "MemoryCards"),
            Some("my cards")
        );
        std::fs::write(&path, "[Folders]\nMemoryCards = @ByteArray(unknown)\n")?;
        assert!(read_settings(&path).is_err());
        Ok(())
    }
}
