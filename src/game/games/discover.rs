use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};
use serde_json::json;
use walkdir::WalkDir;

use crate::game::platforms::discovery::{
    self as platform_discovery, DiscoveredGame, DiscoveryEvent, DiscoverySource,
};
use crate::menu_utils::StreamingMenuItem;
use crate::ui::catppuccin::{colors, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::prelude::{Level, OutputFormat, emit, get_output_format};
use crate::ui::preview::FzfPreview;

use super::manager::GameCreationContext;

const DISCOVERY_CACHE_VERSION: u32 = 1;
const DISCOVERY_CACHE_FILE: &str = "game-discovery-cache.json";
const DISCOVERY_CACHE_MAX_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredGameRecord {
    pub name: String,
    pub platform: String,
    pub platform_short: String,
    pub unique_key: String,
    pub save_path: String,
    pub game_path: Option<String>,
    pub launch_command: Option<String>,
    pub existing: bool,
    pub tracked_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MenuSelectionPayload {
    pub existing: bool,
    pub display_name: Option<String>,
    pub tracked_name: Option<String>,
    pub save_path: Option<String>,
    pub launch_command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedDiscoveredGame {
    record: DiscoveredGameRecord,
    preview_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiscoveryCacheFile {
    version: u32,
    entries: Vec<CachedDiscoveredGame>,
}

pub fn list_discovered_games(
    sources: &[DiscoverySource],
    scan_path: Option<&str>,
    use_cache: bool,
) -> Result<()> {
    match get_output_format() {
        OutputFormat::Json => emit_discovered_games_as_json(sources, scan_path, use_cache),
        OutputFormat::Text => emit_discovered_games_as_text(sources, scan_path, use_cache),
    }
}

pub fn print_streaming_menu_rows(
    sources: &[DiscoverySource],
    scan_path: Option<&str>,
    use_cache: bool,
) -> Result<()> {
    let mut out = io::BufWriter::new(io::stdout());

    stream_discovered_records(
        sources,
        scan_path,
        use_cache,
        |_, _, _| Ok(()),
        |game| {
            writeln!(
                out,
                "{}",
                StreamingMenuItem::new(
                    "discovered",
                    game.record.unique_key.clone(),
                    discovered_menu_display(
                        game.record
                            .tracked_name
                            .as_deref()
                            .unwrap_or(game.record.name.as_str()),
                        &game.record.platform_short,
                        game.record.existing,
                    ),
                    MenuSelectionPayload {
                        existing: game.record.existing,
                        display_name: Some(game.record.name.clone()),
                        tracked_name: game.record.tracked_name.clone(),
                        save_path: Some(game.record.save_path.clone()),
                        launch_command: game.record.launch_command.clone(),
                    },
                )
                .preview(FzfPreview::Text(game.preview_text.clone()))
                .encode()?
            )?;
            out.flush()?;
            Ok(())
        },
    )?;

    Ok(())
}

fn load_discovered_games_with_preview(
    sources: &[DiscoverySource],
    scan_path: Option<&str>,
    use_cache: bool,
) -> Result<Vec<DiscoveredGameWithPreview>> {
    let mut records = Vec::new();
    stream_discovered_records(
        sources,
        scan_path,
        use_cache,
        |_, _, _| Ok(()),
        |game| {
            records.push(game);
            Ok(())
        },
    )?;

    records.sort_by(|a, b| {
        a.record
            .name
            .to_lowercase()
            .cmp(&b.record.name.to_lowercase())
    });

    Ok(records)
}

fn emit_discovered_games_as_json(
    sources: &[DiscoverySource],
    scan_path: Option<&str>,
    use_cache: bool,
) -> Result<()> {
    let discovered = load_discovered_games_with_preview(sources, scan_path, use_cache)?;
    let games_json: Vec<serde_json::Value> = discovered
        .iter()
        .map(|game| {
            json!({
                "name": game.record.name,
                "platform": game.record.platform,
                "platform_short": game.record.platform_short,
                "unique_key": game.record.unique_key,
                "save_path": game.record.save_path,
                "game_path": game.record.game_path,
                "launch_command": game.record.launch_command,
                "existing": game.record.existing,
                "tracked_name": game.record.tracked_name,
            })
        })
        .collect();

    emit(
        Level::Info,
        "game.discover",
        if discovered.is_empty() {
            "No discoverable games found."
        } else {
            "Discovered games."
        },
        Some(json!({
            "count": discovered.len(),
            "games": games_json,
        })),
    );

    Ok(())
}

fn emit_discovered_games_as_text(
    sources: &[DiscoverySource],
    scan_path: Option<&str>,
    use_cache: bool,
) -> Result<()> {
    let progress = create_discovery_progress(platform_discovery::active_sources(sources).len());
    let mut count = 0usize;

    stream_discovered_records(
        sources,
        scan_path,
        use_cache,
        |index, total, message| {
            progress.set_length(total.max(1) as u64);
            progress.set_position(index as u64);
            progress.set_message(message.to_string());
            Ok(())
        },
        |game| {
            count += 1;
            progress.println(render_discovered_game(&game.record));
            Ok(())
        },
    )?;

    progress.finish_and_clear();
    if count == 0 {
        emit(
            Level::Info,
            "game.discover",
            "No discoverable games found.",
            Some(json!({ "count": 0, "games": [] })),
        );
    } else {
        emit(
            Level::Info,
            "game.discover",
            &format!(
                "Discovered {count} game{}.",
                if count == 1 { "" } else { "s" }
            ),
            Some(json!({ "count": count })),
        );
    }

    Ok(())
}

fn create_discovery_progress(source_count: usize) -> ProgressBar {
    let total = source_count.max(1) as u64;
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .unwrap()
            .tick_chars("-\\|/")
            .progress_chars("#>-"),
    );
    pb.set_message("Preparing discovery");
    pb.enable_steady_tick(std::time::Duration::from_millis(100));
    pb
}

fn render_discovered_game(game: &DiscoveredGameRecord) -> String {
    let mut text = format!(
        "- {} ({})\n  Save path: {}\n",
        game.name, game.platform, game.save_path
    );
    if let Some(game_path) = &game.game_path {
        text.push_str(&format!("  Game path: {}\n", game_path));
    }
    if let Some(launch_command) = &game.launch_command {
        text.push_str(&format!("  Launch command: {}\n", launch_command));
    }
    if let Some(tracked_name) = &game.tracked_name {
        text.push_str(&format!("  Tracked as: {}\n", tracked_name));
    }
    text
}

fn stream_discovered_records<F, G>(
    sources: &[DiscoverySource],
    scan_path: Option<&str>,
    use_cache: bool,
    mut on_progress: F,
    mut on_game: G,
) -> Result<()>
where
    F: FnMut(usize, usize, &str) -> Result<()>,
    G: FnMut(DiscoveredGameWithPreview) -> Result<()>,
{
    let context = GameCreationContext::load().ok();
    let mut seen_save_paths = HashSet::new();
    let scan_root = scan_path
        .map(expand_scan_path)
        .transpose()?
        .map(PathBuf::from);
    let mut cache_entries = if use_cache {
        load_valid_cached_entries()?
            .into_iter()
            .map(|entry| (entry.record.save_path.clone(), entry))
            .collect()
    } else {
        HashMap::new()
    };

    if use_cache {
        let cached_snapshot: Vec<_> = cache_entries.values().cloned().collect();
        for cached in cached_snapshot {
            let runtime_game = cached_into_runtime_game(cached, context.as_ref());
            if cached_record_matches_request(&runtime_game.record, sources, scan_root.as_deref())
                && seen_save_paths.insert(runtime_game.record.save_path.clone())
            {
                on_game(runtime_game)?;
            }
        }
    }

    let mut emit_game = |game: DiscoveredGameWithPreview| {
        if seen_save_paths.insert(game.record.save_path.clone()) {
            cache_entries.insert(
                game.record.save_path.clone(),
                CachedDiscoveredGame {
                    record: game.record.clone(),
                    preview_text: game.preview_text.clone(),
                },
            );
            on_game(game)?;
        }
        Ok(())
    };

    if let Some(scan_path) = scan_path {
        let root = PathBuf::from(
            scan_root
                .clone()
                .unwrap_or_else(|| PathBuf::from(scan_path)),
        );
        if let Some(prefix) = find_prefix_root(&root) {
            on_progress(0, 1, &format!("Scanning Wine prefix {}", prefix.display()))?;
            let result = stream_generic_prefix_records(&prefix, context.as_ref(), emit_game);
            if use_cache {
                save_discovery_cache(cache_entries.into_values().collect())?;
            }
            return result;
        }

        on_progress(0, 1, &format!("Scanning {}", root.display()))?;
        let mut known_prefixes = Vec::new();

        let result = platform_discovery::discover_selected_events(sources, |event| match event {
            DiscoveryEvent::SourceStarted { .. } => Ok(()),
            DiscoveryEvent::GameFound(game) => {
                let record = into_record_with_preview(game, context.as_ref());
                if record_is_under_root(&record.record, &root) {
                    if let Some(prefix_path) = record_prefix_path(&record.record) {
                        known_prefixes.push(prefix_path);
                    }
                    emit_game(record)?;
                }
                Ok(())
            }
        });
        result?;

        for prefix in find_prefixes_under_root(&root) {
            if known_prefixes.iter().any(|known| known == &prefix) {
                continue;
            }
            stream_generic_prefix_records(&prefix, context.as_ref(), &mut emit_game)?;
        }

        if use_cache {
            save_discovery_cache(cache_entries.into_values().collect())?;
        }
        return Ok(());
    }

    let result = platform_discovery::discover_selected_events(sources, |event| match event {
        DiscoveryEvent::SourceStarted {
            index,
            total,
            label,
            ..
        } => on_progress(index, total, label),
        DiscoveryEvent::GameFound(game) => {
            emit_game(into_record_with_preview(game, context.as_ref()))
        }
    });

    result?;
    if use_cache {
        save_discovery_cache(cache_entries.into_values().collect())?;
    }
    Ok(())
}

fn expand_scan_path(scan_path: &str) -> Result<String> {
    shellexpand::full(scan_path)
        .map_err(|e| anyhow::anyhow!("Failed to expand path '{}': {}", scan_path, e))
        .map(|value| value.into_owned())
}

fn find_prefix_root(path: &Path) -> Option<PathBuf> {
    let mut current = path.to_path_buf();

    loop {
        if current.join("drive_c").is_dir() {
            return Some(current);
        }

        if !current.pop() {
            break;
        }
    }

    None
}

fn stream_generic_prefix_records<F>(
    prefix: &Path,
    context: Option<&GameCreationContext>,
    mut on_game: F,
) -> Result<()>
where
    F: FnMut(DiscoveredGameWithPreview) -> Result<()>,
{
    crate::game::platforms::discovery::wine::stream_discover_wine_games_in_prefix(
        prefix,
        |game| {
            let save_path = game.save_path.clone();
            let existing_name =
                context.and_then(|ctx| find_existing_game_for_save(save_path.as_path(), ctx));

            let mut game = game;
            if let Some(existing_name) = existing_name {
                game.set_existing(existing_name);
            }

            on_game(into_record_with_preview(Box::new(game), context))?;
            Ok(())
        },
    )?;

    Ok(())
}

fn record_is_under_root(record: &DiscoveredGameRecord, root: &Path) -> bool {
    record_prefix_path(record).is_some_and(|path| path.starts_with(root))
        || Path::new(&record.save_path).starts_with(root)
}

fn record_prefix_path(record: &DiscoveredGameRecord) -> Option<PathBuf> {
    record
        .game_path
        .as_deref()
        .map(PathBuf::from)
        .filter(|path| path.join("drive_c").is_dir())
}

fn into_record_with_preview(
    mut game: Box<dyn DiscoveredGame>,
    context: Option<&GameCreationContext>,
) -> DiscoveredGameWithPreview {
    if let Some(existing_name) =
        context.and_then(|ctx| find_existing_game_for_save(game.save_path().as_path(), ctx))
    {
        game.set_existing(existing_name);
    }

    DiscoveredGameWithPreview {
        preview_text: preview_to_text(game.build_preview()),
        record: DiscoveredGameRecord {
            name: game.display_name().to_string(),
            platform: game.platform_name().to_string(),
            platform_short: game.platform_short().to_string(),
            unique_key: game.unique_key(),
            save_path: game.save_path().to_string_lossy().to_string(),
            game_path: game
                .game_path()
                .map(|path| path.to_string_lossy().to_string()),
            launch_command: game.build_launch_command(),
            existing: game.is_existing(),
            tracked_name: game.tracked_name().map(ToOwned::to_owned),
        },
    }
}

fn cached_into_runtime_game(
    mut cached: CachedDiscoveredGame,
    context: Option<&GameCreationContext>,
) -> DiscoveredGameWithPreview {
    cached.record.existing = false;
    cached.record.tracked_name = None;

    if let Some(existing_name) = context
        .and_then(|ctx| find_existing_game_for_save(Path::new(&cached.record.save_path), ctx))
    {
        cached.record.existing = true;
        cached.record.tracked_name = Some(existing_name);
    }

    DiscoveredGameWithPreview {
        record: cached.record,
        preview_text: cached.preview_text,
    }
}

fn discovery_cache_path() -> Result<PathBuf> {
    let mut path = dirs::cache_dir().context("Unable to resolve cache directory")?;
    path.push("instant");
    fs::create_dir_all(&path).context("Failed to create discovery cache directory")?;
    Ok(path.join(DISCOVERY_CACHE_FILE))
}

fn clear_discovery_cache_path(path: &Path) {
    let _ = fs::remove_file(path);
}

fn read_cache_file(path: &Path) -> Result<Option<DiscoveryCacheFile>> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).context("Failed to stat discovery cache"),
    };

    if metadata.len() > DISCOVERY_CACHE_MAX_BYTES {
        clear_discovery_cache_path(path);
        return Ok(None);
    }

    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).context("Failed to read discovery cache"),
    };

    let cache = match serde_json::from_slice::<DiscoveryCacheFile>(&bytes) {
        Ok(cache) if cache.version == DISCOVERY_CACHE_VERSION => cache,
        Ok(_) | Err(_) => {
            clear_discovery_cache_path(path);
            return Ok(None);
        }
    };

    Ok(Some(cache))
}

fn save_discovery_cache(entries: Vec<CachedDiscoveredGame>) -> Result<()> {
    let path = discovery_cache_path()?;
    let cache = DiscoveryCacheFile {
        version: DISCOVERY_CACHE_VERSION,
        entries,
    };
    let bytes = serde_json::to_vec(&cache).context("Failed to serialize discovery cache")?;

    if bytes.len() as u64 > DISCOVERY_CACHE_MAX_BYTES {
        clear_discovery_cache_path(&path);
        return Ok(());
    }

    fs::write(&path, bytes).context("Failed to write discovery cache")?;
    Ok(())
}

fn is_cached_record_valid(record: &DiscoveredGameRecord) -> bool {
    if !Path::new(&record.save_path).exists() {
        return false;
    }

    record
        .game_path
        .as_deref()
        .is_none_or(|path| Path::new(path).exists())
}

fn load_valid_cached_entries() -> Result<Vec<CachedDiscoveredGame>> {
    let path = discovery_cache_path()?;
    let Some(cache) = read_cache_file(&path)? else {
        return Ok(Vec::new());
    };

    let mut valid_entries = Vec::new();
    let mut changed = false;

    for entry in cache.entries {
        if is_cached_record_valid(&entry.record) {
            valid_entries.push(entry);
        } else {
            changed = true;
        }
    }

    if changed {
        save_discovery_cache(valid_entries.clone())?;
    }

    Ok(valid_entries)
}

fn cached_record_matches_request(
    record: &DiscoveredGameRecord,
    sources: &[DiscoverySource],
    scan_root: Option<&Path>,
) -> bool {
    if !sources.is_empty() {
        let source_matches = sources
            .iter()
            .copied()
            .any(|source| record_matches_source(record, source));
        if !source_matches {
            return false;
        }
    }

    scan_root.is_none_or(|root| record_is_under_root(record, root))
}

fn record_matches_source(record: &DiscoveredGameRecord, source: DiscoverySource) -> bool {
    match source {
        DiscoverySource::Switch => record.platform_short == "Switch",
        DiscoverySource::Ps2 => record.platform_short == "PS2",
        DiscoverySource::Ps1 => record.platform_short == "PS1",
        DiscoverySource::ThreeDs => record.platform_short == "3DS",
        DiscoverySource::Epic => record.platform_short == "Epic",
        DiscoverySource::Steam => record.platform_short == "Steam",
        DiscoverySource::Faugus => record.platform_short == "Faugus",
        DiscoverySource::Wine => record.platform_short == "Wine",
    }
}

fn find_prefixes_under_root(root: &Path) -> Vec<PathBuf> {
    let mut prefixes = Vec::new();

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_dir())
    {
        let path = entry.path();
        if path.join("drive_c").is_dir() {
            prefixes.push(path.to_path_buf());
        }
    }

    prefixes.sort();
    prefixes.dedup();
    prefixes
}

fn find_existing_game_for_save(
    save_path: &std::path::Path,
    context: &GameCreationContext,
) -> Option<String> {
    context
        .installations
        .installations
        .iter()
        .find(|inst| inst.save_path.as_path() == save_path)
        .map(|inst| inst.game_name.0.clone())
}

fn discovered_menu_display(display_name: &str, platform_short: &str, existing: bool) -> String {
    let icon = if existing {
        format_icon_colored(NerdFont::Gamepad, colors::MAUVE)
    } else {
        match platform_short {
            "Switch" => format_icon_colored(NerdFont::Gamepad, colors::GREEN),
            "PS2" | "PS1" => format_icon_colored(NerdFont::Disc, colors::SAPPHIRE),
            "3DS" => format_icon_colored(NerdFont::Gamepad, colors::YELLOW),
            "Epic" => format_icon_colored(NerdFont::Windows, colors::BLUE),
            "Steam" => format_icon_colored(NerdFont::Steam, colors::SAPPHIRE),
            _ => format_icon_colored(NerdFont::Gamepad, colors::GREEN),
        }
    };

    format!("{icon} {display_name} ({platform_short})")
}

fn preview_to_text(preview: FzfPreview) -> String {
    match preview {
        FzfPreview::Text(text) => text,
        FzfPreview::Command(command) => command,
        FzfPreview::None => String::new(),
    }
}

#[derive(Clone)]
struct DiscoveredGameWithPreview {
    record: DiscoveredGameRecord,
    preview_text: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu_utils::StreamingMenuItem;

    #[test]
    fn menu_fields_are_sanitized() {
        let row = StreamingMenuItem::new("a\tb\nc\rd", "key", "display", serde_json::json!({}))
            .encode()
            .unwrap();
        assert!(row.starts_with("a b c d\t"));
    }

    #[test]
    fn menu_row_encodes_payload() {
        let row = StreamingMenuItem::new(
            "manual",
            "manual",
            "display",
            MenuSelectionPayload {
                existing: false,
                display_name: Some("Game".to_string()),
                tracked_name: None,
                save_path: Some("/tmp/save".to_string()),
                launch_command: Some("run".to_string()),
            },
        )
        .preview(FzfPreview::Text("preview".to_string()))
        .encode()
        .unwrap();

        let fields: Vec<&str> = row.split('\t').collect();
        assert_eq!(fields.len(), 6);
    }

    #[test]
    fn find_prefix_root_walks_up_to_drive_c() {
        let prefix = tempfile::tempdir().unwrap();
        let nested = prefix
            .path()
            .join("drive_c")
            .join("users")
            .join("steamuser")
            .join("AppData");
        std::fs::create_dir_all(&nested).unwrap();

        let resolved = find_prefix_root(&nested).unwrap();
        assert_eq!(resolved, prefix.path());
    }

    #[test]
    fn invalid_cache_file_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("game-discovery-cache.json");
        std::fs::write(&path, b"{not json").unwrap();

        let loaded = read_cache_file(&path).unwrap();

        assert!(loaded.is_none());
        assert!(!path.exists());
    }

    #[test]
    fn stale_cached_record_is_invalid() {
        let record = DiscoveredGameRecord {
            name: "Game".to_string(),
            platform: "Wine".to_string(),
            platform_short: "Wine".to_string(),
            unique_key: "wine:test".to_string(),
            save_path: "/definitely/missing".to_string(),
            game_path: None,
            launch_command: None,
            existing: false,
            tracked_name: None,
        };

        assert!(!is_cached_record_valid(&record));
    }

    #[test]
    fn cached_record_respects_source_filter() {
        let record = DiscoveredGameRecord {
            name: "Sable".to_string(),
            platform: "Epic Games".to_string(),
            platform_short: "Epic".to_string(),
            unique_key: "epic:test".to_string(),
            save_path: "/tmp/save".to_string(),
            game_path: None,
            launch_command: None,
            existing: false,
            tracked_name: None,
        };

        assert!(cached_record_matches_request(
            &record,
            &[DiscoverySource::Epic],
            None
        ));
        assert!(!cached_record_matches_request(
            &record,
            &[DiscoverySource::Steam],
            None
        ));
    }
}
