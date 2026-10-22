//! Inspect PCSX2's effective card directory and possible per-game card overrides.
//! Relevant upstream sources are linked in docs/game-save-collections.md.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};

use crate::game::launch_command::{EmulatorLaunchCommand, EmulatorLauncher};
use crate::game::ownership::{SaveOwner, normalized_path};

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
        if !matches!(section.as_str(), "MemoryCards" | "Folders") {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .context("Unrecognized PCSX2 save setting; refusing to guess")?;
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

/// Ask the selected executable to resolve its own data root. This also handles
/// portable AppImages, sandbox XDG overrides, and version-dependent -datapath
/// semantics without reproducing PCSX2's directory-selection implementation.
pub fn data_root(command: &EmulatorLaunchCommand) -> Result<PathBuf> {
    use std::process::{Command, Stdio};
    use std::time::Duration;
    use wait_timeout::ChildExt;
    let mut probe = match &command.launcher {
        EmulatorLauncher::Native { command } => Command::new(command),
        EmulatorLauncher::AppImage { path } => Command::new(path),
        EmulatorLauncher::Flatpak { app_id } => {
            let mut probe = Command::new("flatpak");
            probe.args(["run", "--env=QT_QPA_PLATFORM=offscreen", app_id]);
            probe
        }
    };
    probe
        .args(["-testconfig", "-earlyconsolelog"])
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("NO_COLOR", "1");
    if let Some(path) = &command.options.data_path {
        probe.arg("-datapath").arg(path);
    }
    let output = tempfile::tempfile()?;
    probe
        .stdout(Stdio::from(output.try_clone()?))
        .stderr(Stdio::from(output.try_clone()?))
        .stdin(Stdio::null());
    let mut child = probe
        .spawn()
        .context("Starting PCSX2 configuration inspection")?;
    let status = match child.wait_timeout(Duration::from_secs(15))? {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            bail!("PCSX2 configuration inspection timed out; no saves were restored");
        }
    };
    ensure!(
        status.success(),
        "PCSX2 configuration inspection failed ({status}); configure the emulator before launching with automatic save sync"
    );
    use std::io::{Read, Seek};
    let mut output = output;
    output.rewind()?;
    let mut text = String::new();
    output.take(1024 * 1024).read_to_string(&mut text)?;
    let ansi = regex::Regex::new(r"\x1b\[[0-9;]*m")?;
    let clean = ansi.replace_all(&text, "");
    let roots: std::collections::BTreeSet<_> = clean
        .lines()
        .filter_map(|line| {
            line.split_once("DataRoot Directory: ")
                .map(|(_, path)| path.trim())
        })
        .collect();
    ensure!(
        roots.len() == 1,
        "PCSX2 did not report one unambiguous data directory; no saves were restored. This PCSX2 version must support -testconfig and DataRoot logging"
    );
    let root = Path::new(roots.first().context("No PCSX2 data root")?);
    ensure!(
        root.is_absolute(),
        "PCSX2 reported a relative data root; no saves were restored"
    );
    normalized_path(root)
}

pub fn verify(command: &EmulatorLaunchCommand, owner: &SaveOwner) -> Result<PathBuf> {
    let root = data_root(command)?;
    let base = read_settings(&root.join("inis/PCSX2.ini"))?;
    let mut variants = Vec::new();
    if let Some(path) = &command.options.game_config {
        variants.push(read_settings(path)?);
    } else {
        // Without reading the disc's serial/CRC, inspect every possible override.
        // This is conservative: never guess which per-game file matches a ROM.
        let directory = folder(&base, &root, "GameSettings", "gamesettings");
        match std::fs::read_dir(&directory) {
            Ok(entries) => {
                for entry in entries {
                    let path = entry?.path();
                    if path
                        .extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("ini"))
                    {
                        variants.push(read_settings(&path)?);
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("Inspecting PCSX2 game settings"),
        }
    }
    validate_cards(&root, &base, &variants, owner)?;
    Ok(root)
}

fn validate_cards(
    root: &Path,
    base: &Settings,
    variants: &[Settings],
    owner: &SaveOwner,
) -> Result<()> {
    let owned_path = normalized_path(owner.save_path.as_path())?;
    for override_settings in std::iter::once(None).chain(variants.iter().map(Some)) {
        let mut effective = base.clone();
        if let Some(settings) = override_settings {
            effective.extend(settings.clone());
        }
        let cards = folder(&effective, root, "MemoryCards", "memcards");
        let mut enabled = 0;
        let slots = [
            "Slot1",
            "Slot2",
            "Multitap1_Slot2",
            "Multitap1_Slot3",
            "Multitap1_Slot4",
            "Multitap2_Slot2",
            "Multitap2_Slot3",
            "Multitap2_Slot4",
        ];
        for (index, slot) in slots.iter().enumerate() {
            let enabled_value = setting(&effective, "MemoryCards", &format!("{slot}_Enable"))
                .unwrap_or(if index < 2 { "true" } else { "false" });
            let active = match enabled_value {
                "true" | "1" => true,
                "false" | "0" => false,
                _ => bail!("Unrecognized PCSX2 card enable setting: {enabled_value}"),
            };
            if !active {
                continue;
            }
            enabled += 1;
            let default = format!("Mcd{:03}.ps2", index + 1);
            let filename =
                setting(&effective, "MemoryCards", &format!("{slot}_Filename")).unwrap_or(&default);
            ensure!(
                !filename.is_empty(),
                "Enabled PCSX2 slot has no card filename"
            );
            let card = normalized_path(&cards.join(filename))?;
            let covered = if owner.save_path_type.is_file() {
                card == owned_path
            } else {
                card.starts_with(&owned_path)
            };
            ensure!(
                covered,
                "PCSX2 launch mapping mismatch: card {} is outside {}. No saves were restored. Bind the full card directory or configure all enabled slots to use the declared owner",
                card.display(),
                owned_path.display()
            );
        }
        ensure!(
            enabled > 0,
            "PCSX2 has no enabled card slots; cannot verify this save owner"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::TildePath;
    use crate::game::config::{OwnedSaves, PathContentKind};
    use crate::game::ownership::OwnerId;

    fn owner(path: PathBuf, kind: PathContentKind) -> SaveOwner {
        SaveOwner {
            id: OwnerId::Group("ps2".into()),
            name: "PS2 cards".into(),
            members: vec!["A".into(), "B".into()],
            saves: OwnedSaves::new(TildePath::new(path), kind),
        }
    }

    #[test]
    fn directory_covers_both_slots_folder_cards_and_future_cards() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let base = Settings::new();
        let owner = owner(temp.path().join("memcards"), PathContentKind::Directory);
        validate_cards(temp.path(), &base, &[], &owner)?;
        let mut folder_card = Settings::new();
        folder_card.insert(
            ("MemoryCards".into(), "Slot1_Filename".into()),
            "folder-card".into(),
        );
        validate_cards(temp.path(), &base, &[folder_card], &owner)?;
        Ok(())
    }

    #[test]
    fn outside_cards_and_per_game_directory_overrides_block_restore() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let owner = owner(temp.path().join("memcards"), PathContentKind::Directory);
        let mut override_settings = Settings::new();
        override_settings.insert(
            ("MemoryCards".into(), "Slot2_Filename".into()),
            "../outside.ps2".into(),
        );
        assert!(
            validate_cards(temp.path(), &Settings::new(), &[override_settings], &owner).is_err()
        );
        let mut override_settings = Settings::new();
        override_settings.insert(
            ("Folders".into(), "MemoryCards".into()),
            "other-cards".into(),
        );
        assert!(
            validate_cards(temp.path(), &Settings::new(), &[override_settings], &owner).is_err()
        );
        Ok(())
    }

    #[test]
    fn private_card_requires_other_slots_to_be_disabled() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let owner = owner(
            temp.path().join("memcards/Mcd001.ps2"),
            PathContentKind::File,
        );
        assert!(validate_cards(temp.path(), &Settings::new(), &[], &owner).is_err());
        let mut base = Settings::new();
        base.insert(
            ("MemoryCards".into(), "Slot2_Enable".into()),
            "false".into(),
        );
        validate_cards(temp.path(), &base, &[], &owner)?;
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
        std::fs::write(
            &path,
            "[MemoryCards]\nSlot1_Filename = @ByteArray(unknown)\n",
        )?;
        assert!(read_settings(&path).is_err());
        Ok(())
    }
}
