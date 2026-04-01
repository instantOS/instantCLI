use std::collections::HashSet;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;
use base64::{Engine as _, engine::general_purpose};
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};
use serde_json::json;
use walkdir::WalkDir;

use crate::game::platforms::discovery::{
    self as platform_discovery, DiscoveredGame, DiscoveryEvent, DiscoverySource,
};
use crate::ui::catppuccin::{colors, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::prelude::{Level, OutputFormat, emit, get_output_format};
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
    match get_output_format() {
        OutputFormat::Json => emit_discovered_games_as_json(sources, scan_path),
        OutputFormat::Text => emit_discovered_games_as_text(sources, scan_path),
    }
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

    stream_discovered_records(
        sources,
        scan_path,
        |_, _, _| Ok(()),
        |game| {
            writeln!(
                out,
                "{}",
                encode_menu_row(
                    "discovered",
                    &game.record.unique_key,
                    &discovered_menu_display(
                        game.record
                            .tracked_name
                            .as_deref()
                            .unwrap_or(game.record.name.as_str()),
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
                )?
            )?;
            out.flush()?;
            Ok(())
        },
    )?;

    Ok(())
}

pub fn streaming_menu_preview_command() -> &'static str {
    "printf '%s' {4} | base64 -d 2>/dev/null"
}

fn load_discovered_games_with_preview(
    sources: &[DiscoverySource],
    scan_path: Option<&str>,
) -> Result<Vec<DiscoveredGameWithPreview>> {
    let mut records = Vec::new();
    stream_discovered_records(
        sources,
        scan_path,
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
) -> Result<()> {
    let discovered = load_discovered_games_with_preview(sources, scan_path)?;
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
) -> Result<()> {
    let progress = create_discovery_progress(platform_discovery::active_sources(sources).len());
    let mut count = 0usize;

    stream_discovered_records(
        sources,
        scan_path,
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
    mut on_progress: F,
    mut on_game: G,
) -> Result<()>
where
    F: FnMut(usize, usize, &str) -> Result<()>,
    G: FnMut(DiscoveredGameWithPreview) -> Result<()>,
{
    let context = GameCreationContext::load().ok();
    let mut seen_save_paths = HashSet::new();

    let mut emit_game = |game: DiscoveredGameWithPreview| {
        if seen_save_paths.insert(game.record.save_path.clone()) {
            on_game(game)?;
        }
        Ok(())
    };

    if let Some(scan_path) = scan_path {
        let root = PathBuf::from(expand_scan_path(scan_path)?);
        if let Some(prefix) = find_prefix_root(&root) {
            on_progress(0, 1, &format!("Scanning Wine prefix {}", prefix.display()))?;
            return stream_generic_prefix_records(&prefix, context.as_ref(), emit_game);
        }

        on_progress(0, 1, &format!("Scanning {}", root.display()))?;
        let mut known_prefixes = Vec::new();

        platform_discovery::discover_selected_events(sources, |event| match event {
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
        })?;

        for prefix in find_prefixes_under_root(&root) {
            if known_prefixes.iter().any(|known| known == &prefix) {
                continue;
            }
            stream_generic_prefix_records(&prefix, context.as_ref(), &mut emit_game)?;
        }

        return Ok(());
    }

    platform_discovery::discover_selected_events(sources, |event| match event {
        DiscoveryEvent::SourceStarted {
            index,
            total,
            label,
            ..
        } => on_progress(index, total, label),
        DiscoveryEvent::GameFound(game) => {
            emit_game(into_record_with_preview(game, context.as_ref()))
        }
    })
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
            launch_command: None,
            existing: game.is_existing(),
            tracked_name: game.tracked_name().map(ToOwned::to_owned),
        },
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
