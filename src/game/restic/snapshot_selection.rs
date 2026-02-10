use crate::game::config::InstantGameConfig;
use crate::game::restic::tags;
use crate::game::utils::save_files::{
    TimeComparison, compare_snapshot_vs_local, format_system_time_for_display,
    get_save_directory_info,
};
use crate::menu_utils::{FzfSelectable, FzfWrapper};
use crate::restic::wrapper::Snapshot;
use crate::ui::catppuccin::colors;
use crate::ui::prelude::*;
use anyhow::{Context, Result};

/// Get the current system hostname
fn get_current_hostname() -> Option<String> {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
            } else {
                None
            }
        })
}

impl FzfSelectable for Snapshot {
    fn fzf_display_text(&self) -> String {
        let date = format_date(&self.time);
        let host = &self.hostname;

        // Extract game name from tags if available
        let game_name =
            tags::extract_game_name_from_tags(&self.tags).unwrap_or_else(|| "unknown".to_string());

        format!("{game_name} - {date} ({host})")
    }

    fn fzf_key(&self) -> String {
        self.id.clone()
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        // Extract game name from tags
        let game_name =
            tags::extract_game_name_from_tags(&self.tags).unwrap_or_else(|| "unknown".to_string());

        build_snapshot_preview(self, &game_name, None, None)
    }
}

/// Format ISO date string to readable format
fn format_date(iso_date: &str) -> String {
    // Parse the ISO 8601 date and format it nicely
    if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(iso_date) {
        parsed.format("%Y-%m-%d %H:%M:%S").to_string()
    } else {
        iso_date.to_string()
    }
}

/// Format date with "time ago" information
fn format_date_with_time_ago(iso_date: &str) -> (String, String) {
    if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(iso_date) {
        let now = chrono::Utc::now();
        let duration = now.signed_duration_since(parsed.with_timezone(&chrono::Utc));

        let formatted_date = parsed.format("%Y-%m-%d %H:%M:%S").to_string();

        // Add "time ago" formatting
        let time_ago = if duration.num_seconds().abs() < 60 {
            format!("{} seconds ago", duration.num_seconds().abs())
        } else if duration.num_minutes().abs() < 60 {
            format!("{} minutes ago", duration.num_minutes().abs())
        } else if duration.num_hours().abs() < 24 {
            format!("{} hours ago", duration.num_hours().abs())
        } else if duration.num_days().abs() < 7 {
            format!("{} days ago", duration.num_days().abs())
        } else if duration.num_weeks().abs() < 4 {
            format!("{} weeks ago", duration.num_weeks().abs())
        } else {
            format!("{} months ago", duration.num_days().abs() / 30)
        };

        (formatted_date, format!("({time_ago})"))
    } else {
        (iso_date.to_string(), String::new())
    }
}

/// Add header section to preview builder
fn add_preview_header(
    mut builder: PreviewBuilder,
    snapshot: &Snapshot,
    game_name: &str,
) -> PreviewBuilder {
    let (formatted_time, time_ago) = format_date_with_time_ago(&snapshot.time);

    builder = builder
        .header(NerdFont::Folder, "Snapshot Information")
        .field("Game", game_name)
        .field("Host", &snapshot.hostname)
        .field("Created", &formatted_time);

    if !time_ago.is_empty() {
        builder = builder.field_indented("", &time_ago);
    }

    builder.field("Short ID", &snapshot.short_id)
}

/// Add file statistics section to preview builder
fn add_preview_statistics(
    mut builder: PreviewBuilder,
    summary: &crate::restic::wrapper::SnapshotSummary,
) -> PreviewBuilder {
    let total_files = summary.files_new + summary.files_changed + summary.files_unmodified;

    builder = builder
        .blank()
        .separator()
        .line(colors::MAUVE, Some(NerdFont::Chart), "Backup Statistics")
        .blank()
        .field("Total Files", &format_number(total_files));

    if summary.files_new > 0 {
        builder = builder.field_indented("New", &format_number(summary.files_new));
    }
    if summary.files_changed > 0 {
        builder = builder.field_indented("Changed", &format_number(summary.files_changed));
    }
    if summary.files_unmodified > 0 {
        builder = builder.field_indented("Unmodified", &format_number(summary.files_unmodified));
    }

    // Data size
    if summary.data_added > 0 {
        let size_str = format_file_size(summary.data_added);
        builder = builder.field("Data Added", &size_str);
    }

    // Duration
    if let (Ok(start), Ok(end)) = (
        chrono::DateTime::parse_from_rfc3339(&summary.backup_start),
        chrono::DateTime::parse_from_rfc3339(&summary.backup_end),
    ) {
        let duration = end.signed_duration_since(start);
        let duration_secs = duration.num_seconds();
        if duration_secs > 0 {
            let duration_str = format_duration(duration_secs);
            builder = builder.field("Duration", &duration_str);
        }
    }

    builder
}

/// Context for determining snapshot restoration status
struct SnapshotComparisonContext {
    is_same_host: bool,
    is_current_checkpoint: bool,
}

impl SnapshotComparisonContext {
    fn new(
        snapshot_hostname: &str,
        snapshot_id: &str,
        snapshot_short_id: &str,
        nearest_checkpoint: Option<&str>,
    ) -> Self {
        let current_hostname = get_current_hostname();
        let is_same_host = current_hostname.as_deref() == Some(snapshot_hostname);
        let is_current_checkpoint = nearest_checkpoint
            .map(|id| id == snapshot_id || id == snapshot_short_id)
            .unwrap_or(false);

        Self {
            is_same_host,
            is_current_checkpoint,
        }
    }
}

/// Add status line based on time comparison result
fn add_comparison_status(
    builder: PreviewBuilder,
    comparison: &TimeComparison,
    context: &SnapshotComparisonContext,
) -> PreviewBuilder {
    let mut builder = builder;
    match comparison {
        TimeComparison::LocalNewer | TimeComparison::LocalNewerWithinTolerance(_) => {
            builder = builder
                .line(
                    colors::YELLOW,
                    Some(NerdFont::Warning),
                    "STATUS: LOCAL SAVES ARE NEWER",
                )
                .subtext("Restoring would overwrite newer local saves");
        }
        TimeComparison::SnapshotNewer => {
            builder = add_snapshot_newer_status(builder, context);
        }
        TimeComparison::SnapshotNewerWithinTolerance(_) => {
            builder = add_newer_within_tolerance(builder, context);
        }
        TimeComparison::Same => {
            builder = add_same_timestamp_status(builder, context);
        }
        TimeComparison::Error(msg) => {
            builder = builder
                .line(
                    colors::RED,
                    Some(NerdFont::Cross),
                    "STATUS: COMPARISON ERROR",
                )
                .subtext(&format!("Error: {}", truncate_string(msg, 60)));
        }
    }

    builder
}

/// Add status when snapshot is newer
fn add_snapshot_newer_status(
    builder: PreviewBuilder,
    context: &SnapshotComparisonContext,
) -> PreviewBuilder {
    if context.is_current_checkpoint && context.is_same_host {
        builder
            .line(colors::BLUE, Some(NerdFont::Check), "STATUS: CURRENT STATE")
            .subtext("This snapshot matches the current local state")
    } else if context.is_same_host {
        builder
            .line(
                colors::GREEN,
                Some(NerdFont::Check),
                "STATUS: LOCAL UNCHANGED",
            )
            .subtext("Local saves unmodified since this backup")
    } else {
        builder
            .line(
                colors::GREEN,
                Some(NerdFont::Check),
                "STATUS: SNAPSHOT IS NEWER",
            )
            .subtext("Safe to restore (backup contains newer data)")
    }
}

/// Add status when snapshot is newer within tolerance
fn add_newer_within_tolerance(
    builder: PreviewBuilder,
    context: &SnapshotComparisonContext,
) -> PreviewBuilder {
    if context.is_current_checkpoint {
        builder
            .line(colors::BLUE, Some(NerdFont::Check), "STATUS: CURRENT STATE")
            .subtext("This snapshot matches the current local state")
    } else if context.is_same_host {
        builder
            .line(colors::BLUE, Some(NerdFont::Sync), "STATUS: SYNCHRONIZED")
            .subtext("Local saves match backup timestamp")
    } else {
        builder
            .line(
                colors::GREEN,
                Some(NerdFont::Check),
                "STATUS: SNAPSHOT IS NEWER",
            )
            .subtext("Safe to restore (backup contains newer data)")
    }
}

/// Add status when timestamps are the same
fn add_same_timestamp_status(
    builder: PreviewBuilder,
    context: &SnapshotComparisonContext,
) -> PreviewBuilder {
    if context.is_current_checkpoint {
        builder
            .line(colors::BLUE, Some(NerdFont::Check), "STATUS: CURRENT STATE")
            .subtext("This snapshot matches the current local state")
    } else {
        builder
            .line(
                colors::BLUE,
                Some(NerdFont::Clock),
                "STATUS: TIMESTAMPS MATCH",
            )
            .subtext("Local saves match backup timestamp")
    }
}

/// Add comparison details when local files exist
fn add_local_files_comparison(
    mut builder: PreviewBuilder,
    local_info: &crate::game::utils::save_files::SaveDirectoryInfo,
    snapshot_time: &str,
    context: &SnapshotComparisonContext,
) -> PreviewBuilder {
    let file_count_str = format_number(local_info.file_count);
    let size_str = format_file_size(local_info.total_size);
    builder = builder.field(
        "Local Files",
        &format!("{file_count_str} files ({size_str})"),
    );

    if let Some(local_time) = local_info.last_modified {
        let local_time_str = format_system_time_for_display(Some(local_time));
        builder = builder.field("Last Modified", &local_time_str).blank();

        match compare_snapshot_vs_local(snapshot_time, local_time) {
            Ok(comparison) => {
                builder = add_comparison_status(builder, &comparison, context);
            }
            Err(_) => {
                builder = builder
                    .line(
                        colors::RED,
                        Some(NerdFont::Cross),
                        "STATUS: COMPARISON FAILED",
                    )
                    .subtext("Unable to compare timestamps");
            }
        }
    } else {
        builder = builder
            .field("Last Modified", "Unknown")
            .blank()
            .line(
                colors::RED,
                Some(NerdFont::Cross),
                "STATUS: MODIFICATION TIME UNKNOWN",
            )
            .subtext("Cannot determine if local saves are newer");
    }

    builder
}

/// Add local save comparison section to preview builder
fn add_preview_local_comparison(
    mut builder: PreviewBuilder,
    local_save_info: Option<&crate::game::utils::save_files::SaveDirectoryInfo>,
    snapshot_time: &str,
    snapshot_hostname: &str,
    snapshot_id: &str,
    snapshot_short_id: &str,
    nearest_checkpoint: Option<&str>,
) -> PreviewBuilder {
    builder = builder
        .blank()
        .separator()
        .line(
            colors::MAUVE,
            Some(NerdFont::Download),
            "Local Save Comparison",
        )
        .blank();

    let context = SnapshotComparisonContext::new(
        snapshot_hostname,
        snapshot_id,
        snapshot_short_id,
        nearest_checkpoint,
    );

    match local_save_info {
        Some(local_info) if local_info.file_count > 0 => {
            builder = add_local_files_comparison(builder, local_info, snapshot_time, &context);
        }
        Some(_) => {
            builder = builder
                .field("Local Files", "None found")
                .blank()
                .line(
                    colors::GREEN,
                    Some(NerdFont::Check),
                    "STATUS: NO LOCAL SAVES",
                )
                .subtext("Safe to restore (no files to overwrite)");
        }
        None => {
            builder = builder
                .field("Local Files", "Information unavailable")
                .blank()
                .line(
                    colors::RED,
                    Some(NerdFont::Cross),
                    "STATUS: LOCAL SAVE INFO UNKNOWN",
                )
                .subtext("Cannot determine local save status");
        }
    }

    builder
}

/// Add metadata section to preview builder
fn add_preview_metadata(mut builder: PreviewBuilder, snapshot: &Snapshot) -> PreviewBuilder {
    builder = builder
        .blank()
        .separator()
        .line(colors::MAUVE, Some(NerdFont::Tag), "Snapshot Metadata")
        .blank();

    // Tags
    if !snapshot.tags.is_empty() {
        let tags_str = snapshot.tags.join(", ");
        let truncated_tags = truncate_string(&tags_str, 60);
        builder = builder.field("Tags", &truncated_tags);
    } else {
        builder = builder.field("Tags", "None");
    }

    // Full ID for reference
    builder = builder.field("Full ID", &truncate_string(&snapshot.id, 50));

    // Paths
    if !snapshot.paths.is_empty() {
        builder = builder.blank().subtext("Backup Paths:");
        for (i, path) in snapshot.paths.iter().take(5).enumerate() {
            // Limit to 5 paths to prevent overflow
            let truncated_path = truncate_string(path, 70);
            if i == 0 {
                builder = builder.bullet(&truncated_path);
            } else if i == snapshot.paths.len() - 1 || i == 4 {
                builder = builder.bullet(&truncated_path);
            } else {
                builder = builder.bullet(&truncated_path);
            }
        }

        // Show count if there are more paths than displayed
        if snapshot.paths.len() > 5 {
            let remaining = snapshot.paths.len() - 5;
            builder = builder.bullet(&format!("... and {remaining} more paths"));
        }
    } else {
        builder = builder.field("Backup Paths", "None specified");
    }

    builder
}

/// Create preview using PreviewBuilder
fn build_snapshot_preview(
    snapshot: &Snapshot,
    game_name: &str,
    local_save_info: Option<&crate::game::utils::save_files::SaveDirectoryInfo>,
    nearest_checkpoint: Option<&str>,
) -> crate::menu::protocol::FzfPreview {
    let mut builder = PreviewBuilder::new();

    // Header
    builder = add_preview_header(builder, snapshot, game_name);

    // Local save comparison section (only if info provided)
    if local_save_info.is_some() {
        builder = add_preview_local_comparison(
            builder,
            local_save_info,
            &snapshot.time,
            &snapshot.hostname,
            &snapshot.id,
            &snapshot.short_id,
            nearest_checkpoint,
        );
    }

    // File statistics
    if let Some(summary) = &snapshot.summary {
        builder = add_preview_statistics(builder, summary);
    } else {
        builder = builder
            .blank()
            .separator()
            .line(colors::MAUVE, Some(NerdFont::Chart), "Backup Statistics")
            .blank()
            .subtext("No detailed statistics available");
    }

    // Metadata
    builder = add_preview_metadata(builder, snapshot);

    builder.build()
}

/// Format file size for display
fn format_file_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];

    if bytes == 0 {
        return "0 B".to_string();
    }

    let bytes = bytes as f64;
    let base = 1024_f64;
    let i = (bytes.ln() / base.ln()).floor() as usize;
    let i = i.min(UNITS.len() - 1);

    format!("{:.1} {}", bytes / base.powi(i as i32), UNITS[i])
}

/// Format numbers with thousand separators for better readability
fn format_number(n: u64) -> String {
    n.to_string()
        .as_bytes()
        .rchunks(3)
        .rev()
        .map(std::str::from_utf8)
        .collect::<Result<Vec<&str>, _>>()
        .unwrap()
        .join(",")
}

/// Format duration in seconds to a human-readable format
fn format_duration(seconds: i64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        let minutes = seconds / 60;
        let remaining_seconds = seconds % 60;
        if remaining_seconds == 0 {
            format!("{minutes}m")
        } else {
            format!("{minutes}m {remaining_seconds}s")
        }
    } else {
        let hours = seconds / 3600;
        let remaining_minutes = (seconds % 3600) / 60;
        if remaining_minutes == 0 {
            format!("{hours}h")
        } else {
            format!("{hours}h {remaining_minutes}m")
        }
    }
}

/// Truncate a string to a maximum length, adding "..." if truncated
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else if max_len <= 3 {
        "...".to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

/// Select a snapshot interactively with optional local save comparison
/// Returns Some(snapshot_id) if a snapshot was selected, None if cancelled
pub fn select_snapshot_interactive(
    game_name: &str,
    installation: Option<&crate::game::config::GameInstallation>,
) -> Result<Option<String>> {
    let config = InstantGameConfig::load()
        .context("Failed to load game configuration for snapshot selection")?;

    // Use cached snapshots for better performance
    let snapshots = super::cache::get_snapshots_for_game(game_name, &config)
        .context("Failed to get snapshots for game")?;

    if snapshots.is_empty() {
        emit(
            Level::Warn,
            "game.snapshots.none",
            &format!(
                "{} No snapshots found for game '{game_name}'.\n\nMake sure backups have been created for this game.",
                char::from(NerdFont::Warning)
            ),
            None,
        );
        return Ok(None);
    }

    // Get local save information for comparison if installation is provided
    let (local_save_info, nearest_checkpoint) = if let Some(install) = installation {
        (
            get_save_directory_info(install.save_path.as_path()).ok(),
            install.nearest_checkpoint.clone(),
        )
    } else {
        (None, None)
    };

    // Show selection prompt
    // Create wrapper snapshots that include local comparison info
    let enhanced_snapshots: Vec<EnhancedSnapshot> = snapshots
        .into_iter()
        .map(|snapshot| EnhancedSnapshot {
            snapshot,
            local_save_info: local_save_info.clone(),
            game_name: game_name.to_string(),
            nearest_checkpoint: nearest_checkpoint.clone(),
        })
        .collect();

    let selected = FzfWrapper::select_one(enhanced_snapshots)
        .map_err(|e| anyhow::anyhow!("Failed to select snapshot: {}", e))?;

    match selected {
        Some(enhanced) => Ok(Some(enhanced.snapshot.id)),
        None => {
            emit(
                Level::Info,
                "game.snapshots.cancelled",
                &format!("{} No snapshot selected.", char::from(NerdFont::Info)),
                None,
            );
            Ok(None)
        }
    }
}

/// Enhanced snapshot wrapper that includes local save comparison info
#[derive(Clone)]
pub struct EnhancedSnapshot {
    pub snapshot: Snapshot,
    pub local_save_info: Option<crate::game::utils::save_files::SaveDirectoryInfo>,
    pub game_name: String,
    pub nearest_checkpoint: Option<String>,
}

impl FzfSelectable for EnhancedSnapshot {
    fn fzf_display_text(&self) -> String {
        let date = format_date(&self.snapshot.time);
        let host = &self.snapshot.hostname;

        // Add local save comparison indicator if available
        let comparison_indicator = if let Some(ref local_info) = self.local_save_info {
            if local_info.file_count > 0 {
                if let Some(local_time) = local_info.last_modified {
                    match compare_snapshot_vs_local(&self.snapshot.time, local_time) {
                        Ok(TimeComparison::LocalNewer)
                        | Ok(TimeComparison::LocalNewerWithinTolerance(_)) => " LOCAL NEWER",
                        Ok(TimeComparison::SnapshotNewer)
                        | Ok(TimeComparison::SnapshotNewerWithinTolerance(_)) => " SNAPSHOT NEWER",
                        Ok(TimeComparison::Same) => " =SAME TIME",
                        Ok(TimeComparison::Error(_)) => " COMPARE ERROR",
                        Err(_) => " COMPARE ERROR",
                    }
                } else {
                    &format!("{}NO LOCAL TIME", char::from(NerdFont::Warning))
                }
            } else {
                &format!("{}NO LOCAL SAVES", char::from(NerdFont::Folder))
            }
        } else {
            ""
        };

        format!("{date} ({host}){comparison_indicator}")
    }

    fn fzf_key(&self) -> String {
        // Use the display text as the key since that's what fzf passes to the preview script
        self.fzf_display_text()
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        build_snapshot_preview(
            &self.snapshot,
            &self.game_name,
            self.local_save_info.as_ref(),
            self.nearest_checkpoint.as_deref(),
        )
    }
}
