use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use colored::Colorize;
use serde_json::json;
use std::collections::HashMap;

use crate::game::config::{InstallationsConfig, InstantGameConfig};
use crate::game::restic::tags;
use crate::game::utils::save_files::format_file_size;
use crate::game::utils::validation;
use crate::restic::wrapper::{ResticWrapper, Snapshot};
use crate::ui::prelude::*;

pub fn list_snapshots(game_name: Option<String>) -> Result<()> {
    let game_config = InstantGameConfig::load().context("Failed to load game configuration")?;
    validation::check_restic_and_game_manager(&game_config)?;

    let restic = ResticWrapper::new(
        game_config.repo.as_path().to_string_lossy().to_string(),
        game_config.repo_password.clone(),
    )
    .context("Failed to initialize restic wrapper")?;

    let snapshots_json = match &game_name {
        Some(name) => restic
            .list_snapshots_filtered(Some(tags::create_game_tags(name)))
            .with_context(|| format!("Failed to list snapshots for '{name}'"))?,
        None => restic
            .list_snapshots_filtered(Some(vec![tags::INSTANT_GAME_TAG.to_string()]))
            .context("Failed to list game snapshots")?,
    };

    let mut snapshots: Vec<Snapshot> =
        serde_json::from_str(&snapshots_json).context("Failed to parse snapshot data")?;
    snapshots.sort_by_key(|snapshot| std::cmp::Reverse(parse_snapshot_time(&snapshot.time)));

    let installations = InstallationsConfig::load().unwrap_or_default();
    let checkpoints: HashMap<String, String> = installations
        .installations
        .iter()
        .filter_map(|installation| {
            installation
                .nearest_checkpoint
                .as_ref()
                .map(|checkpoint| (installation.game_name.0.clone(), checkpoint.clone()))
        })
        .collect();

    if snapshots.is_empty() {
        let subject = game_name
            .as_ref()
            .map(|name| format!(" for '{name}'"))
            .unwrap_or_default();
        emit(
            Level::Info,
            "game.snapshots.empty",
            &format!(
                "{} No game snapshots found{}.",
                char::from(NerdFont::Info),
                subject
            ),
            Some(json!({
                "game": game_name,
                "count": 0,
                "snapshots": []
            })),
        );
        return Ok(());
    }

    let entries: Vec<SnapshotListEntry> = snapshots
        .iter()
        .map(|snapshot| SnapshotListEntry::from_snapshot(snapshot, &checkpoints))
        .collect();

    emit(
        Level::Info,
        "game.snapshots.list",
        &render_snapshot_list(game_name.as_deref(), &entries),
        Some(json!({
            "game": game_name,
            "count": entries.len(),
            "snapshots": entries.iter().map(SnapshotListEntry::to_json).collect::<Vec<_>>()
        })),
    );

    Ok(())
}

#[derive(Debug)]
struct SnapshotListEntry {
    game_name: Option<String>,
    id: String,
    short_id: String,
    time: String,
    time_display: String,
    hostname: String,
    username: String,
    paths: Vec<String>,
    tags: Vec<String>,
    is_current_checkpoint: bool,
    files_new: Option<u64>,
    files_changed: Option<u64>,
    files_unmodified: Option<u64>,
    total_files_processed: Option<u64>,
    total_bytes_processed: Option<u64>,
    data_added_packed: Option<u64>,
}

impl SnapshotListEntry {
    fn from_snapshot(snapshot: &Snapshot, checkpoints: &HashMap<String, String>) -> Self {
        let game_name = tags::extract_game_name_from_tags(&snapshot.tags);
        let is_current_checkpoint = game_name
            .as_ref()
            .and_then(|name| checkpoints.get(name))
            .map(|checkpoint| checkpoint == &snapshot.id || checkpoint == &snapshot.short_id)
            .unwrap_or(false);

        let summary = snapshot.summary.as_ref();
        Self {
            game_name,
            id: snapshot.id.clone(),
            short_id: snapshot.short_id.clone(),
            time: snapshot.time.clone(),
            time_display: format_snapshot_time(&snapshot.time),
            hostname: snapshot.hostname.clone(),
            username: snapshot.username.clone(),
            paths: snapshot.paths.clone(),
            tags: snapshot.tags.clone(),
            is_current_checkpoint,
            files_new: summary.map(|s| s.files_new),
            files_changed: summary.map(|s| s.files_changed),
            files_unmodified: summary.map(|s| s.files_unmodified),
            total_files_processed: summary.map(|s| s.total_files_processed),
            total_bytes_processed: summary.map(|s| s.total_bytes_processed),
            data_added_packed: summary.map(|s| s.data_added_packed),
        }
    }

    fn to_json(&self) -> serde_json::Value {
        json!({
            "game": self.game_name,
            "id": self.id,
            "short_id": self.short_id,
            "time": self.time,
            "hostname": self.hostname,
            "username": self.username,
            "paths": self.paths,
            "tags": self.tags,
            "is_current_checkpoint": self.is_current_checkpoint,
            "summary": {
                "files_new": self.files_new,
                "files_changed": self.files_changed,
                "files_unmodified": self.files_unmodified,
                "total_files_processed": self.total_files_processed,
                "total_bytes_processed": self.total_bytes_processed,
                "data_added_packed": self.data_added_packed,
            }
        })
    }
}

fn render_snapshot_list(game_name: Option<&str>, snapshots: &[SnapshotListEntry]) -> String {
    let title = match game_name {
        Some(name) => format!("Game Snapshots: {name}"),
        None => "Game Snapshots".to_string(),
    };

    let mut text = String::new();
    text.push_str(&format!("{}\n\n", title.bold().underline()));

    for snapshot in snapshots {
        let checkpoint_marker = if snapshot.is_current_checkpoint {
            format!(" {}", "[current]".green().bold())
        } else {
            String::new()
        };

        let game_prefix = if game_name.is_none() {
            snapshot
                .game_name
                .as_ref()
                .map(|name| format!("{} ", name.cyan().bold()))
                .unwrap_or_default()
        } else {
            String::new()
        };

        text.push_str(&format!(
            "  {} {}{}{} ({})\n",
            char::from(NerdFont::Archive),
            game_prefix,
            snapshot.short_id.yellow(),
            checkpoint_marker,
            snapshot.time_display
        ));
        text.push_str(&format!("     Host: {}\n", snapshot.hostname));

        if let Some(summary) = format_summary(snapshot) {
            text.push_str(&format!("     {summary}\n"));
        }

        if let Some(path) = snapshot.paths.first() {
            text.push_str(&format!("     Path: {}\n", path.blue()));
        }

        text.push('\n');
    }

    text.push_str(&format!(
        "Total: {} snapshot{}",
        snapshots.len().to_string().bold(),
        if snapshots.len() == 1 { "" } else { "s" }
    ));

    text
}

fn format_summary(snapshot: &SnapshotListEntry) -> Option<String> {
    let total = snapshot.total_files_processed?;
    let files_new = snapshot.files_new.unwrap_or(0);
    let files_changed = snapshot.files_changed.unwrap_or(0);
    let files_unmodified = snapshot.files_unmodified.unwrap_or(0);
    let total_size = snapshot
        .total_bytes_processed
        .map(format_file_size)
        .unwrap_or_else(|| "unknown".to_string());
    let packed = snapshot
        .data_added_packed
        .map(format_file_size)
        .unwrap_or_else(|| "unknown".to_string());

    Some(format!(
        "Files: {total} (+{files_new}, ~{files_changed}, ={files_unmodified}) | Size: {total_size} | Added: {packed}"
    ))
}

fn parse_snapshot_time(time: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(time)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| DateTime::<Utc>::from(std::time::UNIX_EPOCH))
}

fn format_snapshot_time(time: &str) -> String {
    DateTime::parse_from_rfc3339(time)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S %:z").to_string())
        .unwrap_or_else(|_| time.to_string())
}
