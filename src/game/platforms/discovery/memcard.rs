//! Shared PS1/PS2 memory-card discovery. A card is a save artifact, not
//! necessarily a single game, so no ROM or launch command is invented here.
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::utils::display_name_from_stem;
use super::{DiscoveredGame, DiscoveryLaunchHint, EmulatorSaveSource};
use crate::common::TildePath;
use crate::game::launch_command::EmulatorPlatform;
use crate::game::platforms::validation::has_valid_extension;
use crate::menu::protocol::FzfPreview;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

#[derive(Debug, Clone)]
pub struct DiscoveredMemcard {
    display_name: String,
    memcard_path: PathBuf,
    platform: EmulatorPlatform,
    source: EmulatorSaveSource,
    tracked_name: Option<String>,
}

impl DiscoveredGame for DiscoveredMemcard {
    fn display_name(&self) -> &str {
        &self.display_name
    }
    fn save_path(&self) -> &PathBuf {
        &self.memcard_path
    }
    fn game_path(&self) -> Option<&PathBuf> {
        None
    }
    fn launch_hint(&self) -> Option<DiscoveryLaunchHint> {
        Some(DiscoveryLaunchHint::Emulator {
            platform: self.platform,
            source: self.source,
            game: None,
        })
    }
    fn platform_name(&self) -> &'static str {
        self.platform.system_name()
    }
    fn platform_short(&self) -> &'static str {
        match self.platform {
            EmulatorPlatform::Pcsx2 => "PS2",
            _ => "PS1",
        }
    }
    fn unique_key(&self) -> String {
        format!(
            "{}:{}|{}",
            self.platform.id(),
            match self.source {
                EmulatorSaveSource::Native => "Native/AppImage",
                EmulatorSaveSource::Flatpak => "Flatpak",
            },
            self.memcard_path.to_string_lossy()
        )
    }
    fn is_existing(&self) -> bool {
        self.tracked_name.is_some()
    }
    fn tracked_name(&self) -> Option<&str> {
        self.tracked_name.as_deref()
    }
    fn set_existing(&mut self, tracked_name: String) {
        self.tracked_name = Some(tracked_name);
    }
    fn build_preview(&self) -> FzfPreview {
        PreviewBuilder::new()
            .header(
                if self.is_existing() {
                    NerdFont::Check
                } else {
                    NerdFont::Disc
                },
                self.tracked_name().unwrap_or(&self.display_name),
            )
            .field("Platform", self.platform_name())
            .field(
                "Source",
                match self.source {
                    EmulatorSaveSource::Native => "Native/AppImage",
                    EmulatorSaveSource::Flatpak => "Flatpak",
                },
            )
            .blank()
            .text(if self.platform == EmulatorPlatform::Pcsx2 {
                "Shared memory-card directory:"
            } else {
                "Memory card:"
            })
            .bullet(&TildePath::new(self.memcard_path.clone()).display_string())
            .blank()
            .subtext(if self.is_existing() {
                "Already tracked — press Enter to open game menu"
            } else {
                "Shared storage — configure once for all participating games"
            })
            .build()
    }
    fn clone_box(&self) -> Box<dyn DiscoveredGame> {
        Box::new(self.clone())
    }
}

fn locations(platform: EmulatorPlatform) -> &'static [(EmulatorSaveSource, &'static str)] {
    match platform {
        EmulatorPlatform::Pcsx2 => &[
            (
                EmulatorSaveSource::Flatpak,
                "~/.var/app/net.pcsx2.PCSX2/config/PCSX2/memcards",
            ),
            (EmulatorSaveSource::Native, "~/.config/PCSX2/memcards"),
        ],
        EmulatorPlatform::DuckStation => &[
            (
                EmulatorSaveSource::Flatpak,
                "~/.var/app/org.duckstation.DuckStation/config/DuckStation/memcards",
            ),
            (
                EmulatorSaveSource::Native,
                "~/.local/share/duckstation/memcards",
            ),
        ],
        _ => &[],
    }
}

pub fn discover(platform: EmulatorPlatform) -> Result<Vec<DiscoveredMemcard>> {
    let mut results = Vec::new();
    for &(source, location) in locations(platform) {
        let mut path = TildePath::from_str(location).into_path_buf();
        if platform == EmulatorPlatform::Pcsx2 {
            let root = if source == EmulatorSaveSource::Native {
                dirs::config_dir()
                    .context("Cannot locate PCSX2 configuration directory")?
                    .join("PCSX2")
            } else {
                path.parent()
                    .context("Memory-card directory has no parent")?
                    .to_path_buf()
            };
            path = crate::game::platforms::pcsx2::configured_card_directory(&root)?;
        }
        results.extend(scan_directory(&path, platform, source)?);
    }
    results.sort_by_cached_key(|card| card.display_name.to_lowercase());
    Ok(results)
}

fn scan_directory(
    dir: &Path,
    platform: EmulatorPlatform,
    source: EmulatorSaveSource,
) -> Result<Vec<DiscoveredMemcard>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Failed to scan memory cards in {}", dir.display()));
        }
    };
    if platform == EmulatorPlatform::Pcsx2 {
        // The directory covers both slots, folder cards, and cards created later.
        drop(entries);
        return Ok(vec![DiscoveredMemcard {
            display_name: match source {
                EmulatorSaveSource::Native => "PS2 memory cards (Native/AppImage)",
                EmulatorSaveSource::Flatpak => "PS2 memory cards (Flatpak)",
            }
            .into(),
            memcard_path: dir.to_path_buf(),
            platform,
            source,
            tracked_name: None,
        }]);
    }
    let extensions: &[&str] = match platform {
        EmulatorPlatform::Pcsx2 => &["ps2"],
        EmulatorPlatform::DuckStation => &["mcd", "mcr"],
        _ => &[],
    };
    let mut results = Vec::new();
    for entry in entries {
        let path = entry
            .with_context(|| format!("Failed to read memory card entry in {}", dir.display()))?
            .path();
        if path.is_file()
            && has_valid_extension(&path, extensions)
            && let Some(display_name) = display_name_from_stem(&path)
        {
            results.push(DiscoveredMemcard {
                display_name,
                memcard_path: path,
                platform,
                source,
                tracked_name: None,
            });
        }
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_only_platform_cards_and_preserves_save_namespace() {
        let temp = tempfile::tempdir().unwrap();
        for name in ["PS2.PS2", "PS1.MCD", "PS1.mcr", "notes.txt"] {
            std::fs::write(temp.path().join(name), b"card").unwrap();
        }
        std::fs::create_dir(temp.path().join("directory.ps2")).unwrap();
        for (platform, count) in [
            (EmulatorPlatform::Pcsx2, 1),
            (EmulatorPlatform::DuckStation, 2),
        ] {
            let mut cards =
                scan_directory(temp.path(), platform, EmulatorSaveSource::Flatpak).unwrap();
            assert_eq!(cards.len(), count);
            assert!(matches!(
                cards[0].launch_hint(),
                Some(DiscoveryLaunchHint::Emulator {
                    source: EmulatorSaveSource::Flatpak,
                    game: None,
                    ..
                })
            ));
            assert!(!cards[0].is_existing());
            cards[0].set_existing("Tracked game".to_string());
            assert_eq!(cards[0].tracked_name(), Some("Tracked game"));
            assert!(cards[0].is_existing());
        }
    }

    #[test]
    fn missing_directory_is_empty_but_invalid_directory_is_an_error() {
        let temp = tempfile::tempdir().unwrap();
        assert!(
            scan_directory(
                &temp.path().join("missing"),
                EmulatorPlatform::Pcsx2,
                EmulatorSaveSource::Native
            )
            .unwrap()
            .is_empty()
        );
        let file = temp.path().join("file");
        std::fs::write(&file, b"file").unwrap();
        assert!(
            scan_directory(&file, EmulatorPlatform::Pcsx2, EmulatorSaveSource::Native).is_err()
        );
    }
}
