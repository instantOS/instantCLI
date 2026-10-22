//! Game images, independently of memory-card ownership. Cache schema from
//! https://github.com/PCSX2/pcsx2/blob/master/pcsx2/GameList.cpp (version 34).
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result, ensure};
use walkdir::WalkDir;

use super::utils::display_name_from_path;
use super::{DiscoveredGame, DiscoveryLaunchHint, EmulatorSaveSource};
use crate::game::launch_command::EmulatorPlatform;
use crate::game::ownership::normalized_path;
use crate::game::platforms::pcsx2::Settings;
use crate::game::platforms::validation::has_valid_extension;
use crate::menu::protocol::FzfPreview;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

const MAX_CACHE_BYTES: u64 = 64 * 1024 * 1024;
const CACHE_SIGNATURE: u32 = 0x45434c47;
const CACHE_VERSION: u32 = 34;

#[derive(Debug, Clone)]
pub struct Pcsx2Game {
    name: String,
    path: PathBuf,
    serial: Option<String>,
    source: EmulatorSaveSource,
    tracked_name: Option<String>,
}

impl DiscoveredGame for Pcsx2Game {
    fn display_name(&self) -> &str {
        &self.name
    }
    fn save_path(&self) -> Option<&PathBuf> {
        None
    }
    fn game_path(&self) -> Option<&PathBuf> {
        Some(&self.path)
    }
    fn platform_name(&self) -> &'static str {
        "PlayStation 2"
    }
    fn platform_short(&self) -> &'static str {
        "PS2"
    }
    fn unique_key(&self) -> String {
        format!("pcsx2:{:?}|{}", self.source, self.path.display())
    }
    fn launch_hint(&self) -> Option<DiscoveryLaunchHint> {
        Some(DiscoveryLaunchHint::Emulator {
            platform: EmulatorPlatform::Pcsx2,
            source: self.source,
            game: Some(self.path.clone()),
        })
    }
    fn is_existing(&self) -> bool {
        self.tracked_name.is_some()
    }
    fn tracked_name(&self) -> Option<&str> {
        self.tracked_name.as_deref()
    }
    fn set_existing(&mut self, name: String) {
        self.tracked_name = Some(name);
    }
    fn build_preview(&self) -> FzfPreview {
        let mut preview = PreviewBuilder::new()
            .header(NerdFont::Gamepad, &self.name)
            .field("Game", &self.path.display().to_string());
        if let Some(serial) = &self.serial {
            preview = preview.field("Serial", serial);
        }
        preview
            .blank()
            .text("Select an individual memory card or shared save group when adding.")
            .build()
    }
    fn clone_box(&self) -> Box<dyn DiscoveredGame> {
        Box::new(self.clone())
    }
}

pub fn discover() -> Result<Vec<Pcsx2Game>> {
    let home = dirs::home_dir().context("Cannot locate home directory")?;
    let native = dirs::config_dir()
        .context("Cannot locate configuration directory")?
        .join("PCSX2");
    let flatpak = home.join(".var/app/net.pcsx2.PCSX2");
    let installations = [
        (
            EmulatorSaveSource::Native,
            native,
            dirs::cache_dir().map(|path| path.join("PCSX2")),
        ),
        (
            EmulatorSaveSource::Flatpak,
            flatpak.join("config/PCSX2"),
            Some(flatpak.join("cache/PCSX2")),
        ),
    ];
    let mut games = Vec::new();
    for (source, root, legacy_cache) in installations {
        match discover_installation(&root, legacy_cache.as_deref(), source) {
            Ok(found) => games.extend(found),
            Err(error) => eprintln!("PCSX2 discovery at {}: {error:#}", root.display()),
        }
    }
    games.sort_by_cached_key(|game| (game.name.to_lowercase(), game.unique_key()));
    Ok(games)
}

fn discover_installation(
    root: &Path,
    legacy_cache: Option<&Path>,
    source: EmulatorSaveSource,
) -> Result<Vec<Pcsx2Game>> {
    let settings = Settings::load(root)?;
    let mut cache_path = settings
        .folder(root, "Cache", "cache")?
        .join("gamelist.cache");
    if settings.values("Folders", "Cache").is_empty()
        && !cache_path.exists()
        && let Some(legacy) = legacy_cache
    {
        cache_path = legacy.join("gamelist.cache");
    }
    // A cache is an optional title hint. Corruption, missing files, and future
    // versions never block directory discovery or trigger emulator execution.
    let cache = match read_cache(&cache_path) {
        Ok(cache) => cache,
        Err(error) => {
            if crate::ui::is_debug_enabled() {
                eprintln!("Ignoring PCSX2 cache {}: {error:#}", cache_path.display());
            }
            HashMap::new()
        }
    };
    let excluded = settings
        .values("GameList", "ExcludedPaths")
        .iter()
        .map(|path| resolve_path(root, path))
        .collect::<Result<Vec<_>>>()?;
    let mut seen = HashSet::new();
    let mut games = Vec::new();
    for (key, recursive) in [("Paths", false), ("RecursivePaths", true)] {
        for directory in settings.values("GameList", key) {
            let directory = resolve_path(root, directory)?;
            if !directory.is_dir() {
                continue;
            }
            let walker = WalkDir::new(&directory)
                .max_depth(if recursive { usize::MAX } else { 1 })
                .into_iter()
                .filter_entry(|entry| {
                    !excluded
                        .iter()
                        .any(|excluded| entry.path().starts_with(excluded))
                });
            for entry in walker {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        eprintln!("Scanning PCSX2 games: {error}");
                        continue;
                    }
                };
                if !entry.path().is_file()
                    || !has_valid_extension(entry.path(), EmulatorPlatform::Pcsx2.extensions())
                {
                    continue;
                }
                let path = normalized_path(entry.path())?;
                if excluded.iter().any(|excluded| path.starts_with(excluded))
                    || !seen.insert(path.clone())
                {
                    continue;
                }
                let metadata = fs::metadata(&path)?;
                let modified = metadata.modified()?.duration_since(UNIX_EPOCH)?.as_secs();
                let cached = cache
                    .get(&path)
                    .filter(|cached| cached.modified == modified);
                if cached.is_some_and(|cached| matches!(cached.kind, 1 | 3)) {
                    continue;
                }
                let name = cached
                    .filter(|cached| !cached.title.trim().is_empty())
                    .map(|cached| cached.title.clone())
                    .unwrap_or_else(|| display_name_from_path(&path));
                games.push(Pcsx2Game {
                    name,
                    path,
                    source,
                    serial: cached.and_then(|cached| {
                        (!cached.serial.is_empty()).then(|| cached.serial.clone())
                    }),
                    tracked_name: None,
                });
            }
        }
    }
    Ok(games)
}

fn resolve_path(root: &Path, value: &str) -> Result<PathBuf> {
    let path = Path::new(value);
    normalized_path(&if path.is_absolute() {
        path.into()
    } else {
        root.join(path)
    })
}

#[derive(Debug)]
struct CacheEntry {
    title: String,
    serial: String,
    modified: u64,
    kind: u8,
}

fn read_cache(path: &Path) -> Result<HashMap<PathBuf, CacheEntry>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_CACHE_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_CACHE_BYTES,
        "PCSX2 cache exceeds size limit"
    );
    parse_cache(&bytes)
}

fn parse_cache(bytes: &[u8]) -> Result<HashMap<PathBuf, CacheEntry>> {
    let mut input = Cursor::new(bytes);
    ensure!(
        u32_value(&mut input)? == CACHE_SIGNATURE,
        "Unknown PCSX2 cache signature"
    );
    ensure!(
        u32_value(&mut input)? == CACHE_VERSION,
        "Unsupported PCSX2 cache version"
    );
    let mut entries = HashMap::new();
    while input.position() < bytes.len() as u64 {
        let path = string_value(&mut input)?;
        let serial = string_value(&mut input)?;
        let title = string_value(&mut input)?;
        let _title_sort = string_value(&mut input)?;
        let _title_en = string_value(&mut input)?;
        let mut tail = [0u8; 23];
        input.read_exact(&mut tail)?;
        let kind = tail[0];
        ensure!(kind <= 3 && tail[22] <= 6, "Invalid PCSX2 cache entry");
        let modified = u64::from_le_bytes(tail[10..18].try_into()?);
        if Path::new(&path).is_absolute() {
            entries.insert(
                normalized_path(Path::new(&path))?,
                CacheEntry {
                    title,
                    serial,
                    modified,
                    kind,
                },
            );
        }
    }
    Ok(entries)
}

fn u32_value(input: &mut Cursor<&[u8]>) -> Result<u32> {
    let mut bytes = [0; 4];
    input.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}
fn string_value(input: &mut Cursor<&[u8]>) -> Result<String> {
    let length = u32_value(input)? as usize;
    ensure!(
        length <= 1024 * 1024 && length as u64 <= input.get_ref().len() as u64 - input.position(),
        "Invalid PCSX2 cache string length"
    );
    let mut bytes = vec![0; length];
    input.read_exact(&mut bytes)?;
    Ok(String::from_utf8(bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache_bytes(path: &Path, title: &str, modified: u64) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(CACHE_SIGNATURE.to_le_bytes());
        bytes.extend(CACHE_VERSION.to_le_bytes());
        for value in [
            path.to_str().unwrap(),
            "SLUS-12345",
            title,
            "sort",
            "English",
        ] {
            bytes.extend((value.len() as u32).to_le_bytes());
            bytes.extend(value.as_bytes());
        }
        bytes.extend([0, 0]);
        bytes.extend(123u64.to_le_bytes());
        bytes.extend(modified.to_le_bytes());
        bytes.extend(0x12345678u32.to_le_bytes());
        bytes.push(5);
        bytes
    }

    #[test]
    fn rejects_unknown_truncated_and_unbounded_cache_data() {
        assert!(parse_cache(&[]).is_err());
        let mut bytes = cache_bytes(Path::new("/games/A.iso"), "Title", 123);
        assert!(parse_cache(&bytes).is_ok());
        assert!(parse_cache(&bytes[..bytes.len() - 1]).is_err());
        bytes[4..8].copy_from_slice(&999u32.to_le_bytes());
        assert!(parse_cache(&bytes).is_err());
        bytes[4..8].copy_from_slice(&CACHE_VERSION.to_le_bytes());
        bytes[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse_cache(&bytes).is_err());
    }

    #[test]
    fn scans_configured_roots_enriches_fresh_cache_and_respects_exclusions() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        for directory in [
            "inis",
            "games/nested",
            "games/skip",
            "other/deep",
            "custom-cache",
        ] {
            fs::create_dir_all(root.join(directory))?;
        }
        for file in [
            "games/A.iso",
            "games/nested/not-recursive.iso",
            "games/skip/hidden.iso",
            "other/deep/B.chd",
            "other/notes.txt",
        ] {
            fs::write(root.join(file), b"image")?;
        }
        fs::write(
            root.join("inis/PCSX2.ini"),
            "[Folders]\nCache = custom-cache\n[GameList]\nPaths = games\nPaths = games\nRecursivePaths = other\nExcludedPaths = games/skip\n",
        )?;
        let path = root.join("games/A.iso");
        let modified = fs::metadata(&path)?
            .modified()?
            .duration_since(UNIX_EPOCH)?
            .as_secs();
        let cache = root.join("custom-cache/gamelist.cache");
        let bytes = cache_bytes(&path, "Database title", modified);
        fs::write(&cache, &bytes)?;
        let games = discover_installation(root, None, EmulatorSaveSource::Native)?;
        assert_eq!(games.len(), 2);
        let game = games.iter().find(|game| game.path == path).unwrap();
        assert_eq!(game.name, "Database title");
        assert_eq!(game.serial.as_deref(), Some("SLUS-12345"));
        assert!(game.save_path().is_none());
        assert_eq!(fs::read(&cache)?, bytes);
        // A stale or unsupported cache cannot hide an image or supply stale titles.
        fs::write(
            &cache,
            cache_bytes(&path, "Stale title", modified.saturating_sub(1)),
        )?;
        assert_eq!(
            discover_installation(root, None, EmulatorSaveSource::Native)?
                .iter()
                .find(|game| game.path == path)
                .unwrap()
                .name,
            "A"
        );
        fs::write(&cache, b"future-or-corrupt")?;
        assert_eq!(
            discover_installation(root, None, EmulatorSaveSource::Native)?.len(),
            2
        );
        Ok(())
    }

    #[test]
    fn recursive_exclusions_and_installation_identity_remain_separate() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("inis"))?;
        fs::create_dir_all(root.join("games/skip"))?;
        fs::write(root.join("games/A.iso"), "image")?;
        fs::write(root.join("games/skip/B.iso"), "image")?;
        fs::write(
            root.join("inis/PCSX2.ini"),
            "[GameList]\nRecursivePaths = games\nExcludedPaths = games/skip\n",
        )?;
        let native = discover_installation(root, None, EmulatorSaveSource::Native)?;
        let flatpak = discover_installation(root, None, EmulatorSaveSource::Flatpak)?;
        assert_eq!(native.len(), 1);
        assert_ne!(native[0].unique_key(), flatpak[0].unique_key());
        assert!(matches!(
            flatpak[0].launch_hint(),
            Some(DiscoveryLaunchHint::Emulator {
                source: EmulatorSaveSource::Flatpak,
                game: Some(_),
                ..
            })
        ));
        Ok(())
    }
}
