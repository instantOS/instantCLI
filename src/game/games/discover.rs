use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;
use base64::{Engine as _, engine::general_purpose};
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};
use serde_json::json;
use walkdir::WalkDir;

use crate::game::platforms::discovery::{self as platform_discovery, DiscoverySource};
use crate::game::platforms::ludusavi::{self, DiscoveredWineSave, choose_primary_save};
use crate::ui::catppuccin::{colors, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::prelude::{Level, emit};
use crate::ui::preview::{FzfPreview, PreviewBuilder};

use super::manager::GameCreationContext;

#[derive(Debug, Clone, Serialize)]
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

pub fn list_discovered_games(sources: &[DiscoverySource], scan_path: Option<&str>) -> Result<()> {
    let discovered = if let Some(scan_path) = scan_path {
        let progress = create_discovery_progress(1);
        progress.set_position(0);
        progress.set_message(format!("Scanning {}", scan_path));
        let discovered = load_discovered_games_from_path(scan_path)?;
        progress.finish_and_clear();
        discovered
    } else {
        let progress = create_discovery_progress(sources.len());
        let discovered = load_discovered_games_with_progress(sources, |index, total, message| {
            progress.set_length(total as u64);
            progress.set_position(index as u64);
            progress.set_message(message.to_string());
        })?;
        progress.finish_and_clear();
        discovered
    };

    let games_json: Vec<serde_json::Value> = discovered
        .iter()
        .map(|game| {
            json!({
                "name": game.name,
                "platform": game.platform,
                "platform_short": game.platform_short,
                "unique_key": game.unique_key,
                "save_path": game.save_path,
                "game_path": game.game_path,
                "launch_command": game.launch_command,
                "existing": game.existing,
                "tracked_name": game.tracked_name,
            })
        })
        .collect();

    let message = if discovered.is_empty() {
        "No discoverable games found.".to_string()
    } else {
        let mut text = String::from("Discovered Games\n\n");
        for game in &discovered {
            text.push_str(&format!(
                "- {} ({})\n  Save path: {}\n",
                game.name, game.platform, game.save_path
            ));
            if let Some(game_path) = &game.game_path {
                text.push_str(&format!("  Game path: {}\n", game_path));
            }
            if let Some(launch_command) = &game.launch_command {
                text.push_str(&format!("  Launch command: {}\n", launch_command));
            }
            if let Some(tracked_name) = &game.tracked_name {
                text.push_str(&format!("  Tracked as: {}\n", tracked_name));
            }
            text.push('\n');
        }
        text.pop();
        text
    };

    emit(
        Level::Info,
        "game.discover",
        &message,
        Some(json!({
            "count": discovered.len(),
            "games": games_json,
        })),
    );

    Ok(())
}

pub fn print_streaming_menu_rows(
    sources: &[DiscoverySource],
    scan_path: Option<&str>,
) -> Result<()> {
    let mut out = io::BufWriter::new(io::stdout());

    writeln!(
        out,
        "{}",
        encode_menu_row(
            "manual",
            "manual",
            &manual_menu_display(),
            &preview_to_text(manual_menu_preview()),
            &MenuSelectionPayload {
                existing: false,
                display_name: None,
                tracked_name: None,
                save_path: None,
                launch_command: None,
            },
        )?
    )?;
    out.flush()?;

    for game in load_discovered_menu_items(sources, scan_path)? {
        writeln!(out, "{}", game)?;
        out.flush()?;
    }

    Ok(())
}

pub fn streaming_menu_preview_command() -> &'static str {
    "printf '%s' {4} | base64 -d 2>/dev/null"
}

fn load_discovered_menu_items(
    sources: &[DiscoverySource],
    scan_path: Option<&str>,
) -> Result<Vec<String>> {
    let discovered = if let Some(scan_path) = scan_path {
        load_discovered_games_from_path_with_preview(scan_path)?
    } else {
        load_discovered_games_with_preview(sources)?
    };
    discovered
        .into_iter()
        .map(|game| {
            let display_name = game
                .record
                .tracked_name
                .as_deref()
                .unwrap_or(game.record.name.as_str());
            encode_menu_row(
                "discovered",
                &game.record.unique_key,
                &discovered_menu_display(
                    display_name,
                    &game.record.platform_short,
                    game.record.existing,
                ),
                &game.preview_text,
                &MenuSelectionPayload {
                    existing: game.record.existing,
                    display_name: Some(game.record.name),
                    tracked_name: game.record.tracked_name,
                    save_path: Some(game.record.save_path),
                    launch_command: game.record.launch_command,
                },
            )
        })
        .collect()
}

pub fn load_discovered_games(sources: &[DiscoverySource]) -> Result<Vec<DiscoveredGameRecord>> {
    Ok(load_discovered_games_with_preview(sources)?
        .into_iter()
        .map(|game| game.record)
        .collect())
}

fn load_discovered_games_from_path(scan_path: &str) -> Result<Vec<DiscoveredGameRecord>> {
    Ok(load_discovered_games_from_path_with_preview(scan_path)?
        .into_iter()
        .map(|game| game.record)
        .collect())
}

fn load_discovered_games_with_preview(
    sources: &[DiscoverySource],
) -> Result<Vec<DiscoveredGameWithPreview>> {
    load_discovered_games_with_preview_and_progress(sources, |_, _, _| {})
}

fn load_discovered_games_from_path_with_preview(
    scan_path: &str,
) -> Result<Vec<DiscoveredGameWithPreview>> {
    let root = PathBuf::from(expand_scan_path(scan_path)?);

    if let Some(prefix) = find_prefix_root(&root) {
        return load_generic_prefix_records(&prefix);
    }

    let mut records = filter_records_to_root(&load_discovered_games_with_preview(&[])?, &root);
    let known_prefixes: Vec<PathBuf> = records
        .iter()
        .filter_map(|record| {
            record
                .record
                .game_path
                .as_deref()
                .map(PathBuf::from)
                .filter(|path| path.join("drive_c").is_dir())
        })
        .collect();

    for prefix in find_prefixes_under_root(&root) {
        if known_prefixes.iter().any(|known| known == &prefix) {
            continue;
        }
        records.extend(load_generic_prefix_records(&prefix)?);
    }

    records.sort_by(|a, b| {
        a.record
            .name
            .to_lowercase()
            .cmp(&b.record.name.to_lowercase())
            .then_with(|| a.record.save_path.cmp(&b.record.save_path))
    });
    records.dedup_by(|a, b| a.record.unique_key == b.record.unique_key);

    Ok(records)
}

fn load_discovered_games_with_progress<F>(
    sources: &[DiscoverySource],
    on_source_start: F,
) -> Result<Vec<DiscoveredGameRecord>>
where
    F: FnMut(usize, usize, &'static str),
{
    Ok(
        load_discovered_games_with_preview_and_progress(sources, on_source_start)?
            .into_iter()
            .map(|game| game.record)
            .collect(),
    )
}

fn load_discovered_games_with_preview_and_progress<F>(
    sources: &[DiscoverySource],
    on_source_start: F,
) -> Result<Vec<DiscoveredGameWithPreview>>
where
    F: FnMut(usize, usize, &'static str),
{
    let default_sources = [
        DiscoverySource::Switch,
        DiscoverySource::Ps2,
        DiscoverySource::Ps1,
        DiscoverySource::ThreeDs,
        DiscoverySource::Epic,
        DiscoverySource::Steam,
    ];
    let active_sources = if sources.is_empty() {
        &default_sources[..]
    } else {
        sources
    };

    let mut discovered =
        platform_discovery::discover_selected_with_progress(active_sources, on_source_start)?;
    let context = GameCreationContext::load().ok();

    let mut records = Vec::with_capacity(discovered.len());
    for mut game in discovered.drain(..) {
        if let Some(existing_name) = context
            .as_ref()
            .and_then(|ctx| find_existing_game_for_save(game.save_path().as_path(), ctx))
        {
            game.set_existing(existing_name);
        }

        records.push(DiscoveredGameWithPreview {
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
        });
    }

    records.sort_by(|a, b| {
        a.record
            .name
            .to_lowercase()
            .cmp(&b.record.name.to_lowercase())
    });

    Ok(records)
}

fn create_discovery_progress(source_count: usize) -> ProgressBar {
    let total = source_count.max(1) as u64;
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("#>-"),
    );
    pb.set_message("Preparing discovery");
    pb
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

fn load_generic_prefix_records(prefix: &Path) -> Result<Vec<DiscoveredGameWithPreview>> {
    let context = GameCreationContext::load().ok();
    let saves = ludusavi::scan_wine_prefix(prefix)?;
    let mut grouped: BTreeMap<String, Vec<DiscoveredWineSave>> = BTreeMap::new();

    for save in saves {
        grouped
            .entry(save.game_name.clone())
            .or_default()
            .push(save);
    }

    let mut records = Vec::new();
    for (game_name, candidates) in grouped {
        let Some(primary_save) = choose_primary_save(candidates) else {
            continue;
        };

        let save_path = PathBuf::from(&primary_save.save_path);
        let existing_name = context
            .as_ref()
            .and_then(|ctx| find_existing_game_for_save(save_path.as_path(), ctx));

        records.push(DiscoveredGameWithPreview {
            preview_text: preview_to_text(
                PreviewBuilder::new()
                    .header(NerdFont::Wine, &game_name)
                    .text("Platform: Wine Prefix")
                    .blank()
                    .text("Prefix:")
                    .bullet(&prefix.to_string_lossy())
                    .blank()
                    .text("Save path:")
                    .bullet(&primary_save.save_path)
                    .build(),
            ),
            record: DiscoveredGameRecord {
                name: game_name.clone(),
                platform: "Wine Prefix".to_string(),
                platform_short: "Wine".to_string(),
                unique_key: format!(
                    "wine:{}|{}",
                    prefix.to_string_lossy(),
                    primary_save.save_path
                ),
                save_path: primary_save.save_path,
                game_path: Some(prefix.to_string_lossy().to_string()),
                launch_command: None,
                existing: existing_name.is_some(),
                tracked_name: existing_name,
            },
        });
    }

    Ok(records)
}

fn filter_records_to_root(
    records: &[DiscoveredGameWithPreview],
    root: &Path,
) -> Vec<DiscoveredGameWithPreview> {
    records
        .iter()
        .filter(|record| {
            record
                .record
                .game_path
                .as_deref()
                .map(Path::new)
                .is_some_and(|path| path.starts_with(root))
                || Path::new(&record.record.save_path).starts_with(root)
        })
        .cloned()
        .collect()
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

fn manual_menu_display() -> String {
    format!(
        "{} Enter a new game manually",
        format_icon_colored(NerdFont::Edit, colors::BLUE)
    )
}

fn manual_menu_preview() -> FzfPreview {
    PreviewBuilder::new()
        .header(NerdFont::Edit, "Manual Entry")
        .text("Enter game details manually.")
        .blank()
        .text("You will be prompted for:")
        .bullet("Game name")
        .bullet("Description (optional)")
        .bullet("Launch command (optional)")
        .bullet("Save data path")
        .build()
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

fn encode_menu_row(
    kind: &str,
    key: &str,
    display: &str,
    preview: &str,
    payload: &MenuSelectionPayload,
) -> Result<String> {
    let payload_json = serde_json::to_vec(payload)?;
    Ok(format!(
        "{}\t{}\t{}\t{}\t{}",
        sanitize_menu_field(kind),
        sanitize_menu_field(key),
        sanitize_menu_field(display),
        general_purpose::STANDARD.encode(preview.as_bytes()),
        general_purpose::STANDARD.encode(payload_json),
    ))
}

fn sanitize_menu_field(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            '\t' | '\n' | '\r' => ' ',
            _ => c,
        })
        .collect()
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
    use crate::common::TildePath;
    use crate::game::utils::path::tilde_display_string;

    #[test]
    fn menu_fields_are_sanitized() {
        assert_eq!(sanitize_menu_field("a\tb\nc\rd"), "a b c d");
    }

    #[test]
    fn menu_row_encodes_payload() {
        let row = encode_menu_row(
            "manual",
            "manual",
            "display",
            "preview",
            &MenuSelectionPayload {
                existing: false,
                display_name: Some("Game".to_string()),
                tracked_name: None,
                save_path: Some("/tmp/save".to_string()),
                launch_command: Some("run".to_string()),
            },
        )
        .unwrap();

        let fields: Vec<&str> = row.split('\t').collect();
        assert_eq!(fields.len(), 5);
    }

    #[test]
    fn preview_command_is_stable() {
        assert!(streaming_menu_preview_command().contains("base64 -d"));
    }

    #[test]
    fn tilde_display_roundtrip_for_manual_preview() {
        let preview = preview_to_text(manual_menu_preview());
        assert!(preview.contains("Manual Entry"));
        let home = dirs::home_dir().unwrap();
        let display = tilde_display_string(&TildePath::new(home));
        assert_eq!(display, "~");
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
}
