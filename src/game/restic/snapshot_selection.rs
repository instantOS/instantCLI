use crate::fzf_wrapper::{FzfSelectable, FzfWrapper};
use crate::game::config::InstantGameConfig;
use crate::game::utils::save_files::{
    TimeComparison, compare_snapshot_vs_local, format_system_time_for_display,
    get_save_directory_info,
};
use crate::restic::wrapper::Snapshot;
use anyhow::{Context, Result};

impl FzfSelectable for Snapshot {
    fn fzf_display_text(&self) -> String {
        let date = format_date(&self.time);
        let host = &self.hostname;

        // Extract game name from tags if available
        let game_name = self
            .tags
            .iter()
            .find(|tag| tag != &"instantgame")
            .map(|s| s.as_str())
            .unwrap_or("unknown");

        format!("{game_name} - {date} ({host})")
    }

    fn fzf_key(&self) -> String {
        self.id.clone()
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        // Extract game name from tags
        let game_name = self
            .tags
            .iter()
            .find(|tag| tag != &"instantgame")
            .map(|s| s.as_str())
            .unwrap_or("unknown");

        let preview_text = create_snapshot_preview(self, game_name);
        crate::menu::protocol::FzfPreview::Text(preview_text)
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
fn format_date_with_time_ago(iso_date: &str) -> String {
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

        format!("{formatted_date}\n({time_ago})")
    } else {
        iso_date.to_string()
    }
}

/// Create header section for snapshot preview
fn create_preview_header(snapshot: &Snapshot, game_name: &str) -> String {
    let formatted_time = format_date_with_time_ago(&snapshot.time);

    format!(
        "🎮 SNAPSHOT INFORMATION\n\
         \n\
         Game:      {}\n\
         Host:      {}\n\
         Created:   {}\n\
         Short ID:  {}\n\n",
        game_name,
        snapshot.hostname,
        formatted_time.lines().next().unwrap_or(""),
        snapshot.short_id
    )
}

/// Create file statistics section for snapshot preview
fn create_preview_statistics(summary: &crate::restic::wrapper::SnapshotSummary) -> String {
    let mut stats = String::new();
    let total_files = summary.files_new + summary.files_changed + summary.files_unmodified;

    stats.push_str("📊 BACKUP STATISTICS\n\n");
    stats.push_str(&format!(
        "Total Files:      {}\n",
        format_number(total_files)
    ));

    if summary.files_new > 0 {
        stats.push_str(&format!(
            "  ├─ New:         {}\n",
            format_number(summary.files_new)
        ));
    }
    if summary.files_changed > 0 {
        stats.push_str(&format!(
            "  ├─ Changed:     {}\n",
            format_number(summary.files_changed)
        ));
    }
    if summary.files_unmodified > 0 {
        stats.push_str(&format!(
            "  └─ Unmodified:  {}\n",
            format_number(summary.files_unmodified)
        ));
    }

    // Data size
    if summary.data_added > 0 {
        let size_str = format_file_size(summary.data_added);
        stats.push_str(&format!("\nData Added:       {size_str}\n"));
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
            stats.push_str(&format!("Duration:         {duration_str}\n"));
        }
    }

    stats.push('\n');

    stats
}

/// Create local save comparison section for snapshot preview
fn create_preview_local_comparison(
    local_save_info: &Option<crate::game::utils::save_files::SaveDirectoryInfo>,
    snapshot_time: &str,
) -> String {
    let mut comparison = String::new();

    comparison.push_str("💾 LOCAL SAVE COMPARISON\n\n");

    if let Some(local_info) = local_save_info {
        if local_info.file_count > 0 {
            let file_count_str = format_number(local_info.file_count);
            let size_str = format_file_size(local_info.total_size);
            comparison.push_str(&format!(
                "Local Files:      {file_count_str} files ({size_str})\n"
            ));

            if let Some(local_time) = local_info.last_modified {
                let local_time_str = format_system_time_for_display(Some(local_time));
                comparison.push_str(&format!("Last Modified:    {local_time_str}\n"));

                // Add comparison result with clear status indication
                match compare_snapshot_vs_local(snapshot_time, local_time) {
                    Ok(TimeComparison::LocalNewer) => {
                        comparison.push_str("\n🔴 STATUS: LOCAL SAVES ARE NEWER\n");
                        comparison
                            .push_str("    ⚠️  Restoring would overwrite newer local saves\n");
                    }
                    Ok(TimeComparison::SnapshotNewer) => {
                        comparison.push_str("\n🟢 STATUS: SNAPSHOT IS NEWER\n");
                        comparison
                            .push_str("    ✅ Safe to restore (backup contains newer data)\n");
                    }
                    Ok(TimeComparison::Same) => {
                        comparison.push_str("\n🟡 STATUS: TIMESTAMPS MATCH\n");
                        comparison.push_str("    📊 Local saves match backup timestamp\n");
                    }
                    Ok(TimeComparison::Error(msg)) => {
                        comparison.push_str("\n🔴 STATUS: COMPARISON ERROR\n");
                        comparison.push_str(&format!("    Error: {}\n", truncate_string(&msg, 60)));
                    }
                    Err(_) => {
                        comparison.push_str("\n🔴 STATUS: COMPARISON FAILED\n");
                        comparison.push_str("    Unable to compare timestamps\n");
                    }
                }
            } else {
                comparison.push_str("Last Modified:    Unknown\n");
                comparison.push_str("\n🔴 STATUS: MODIFICATION TIME UNKNOWN\n");
                comparison.push_str("    Cannot determine if local saves are newer\n");
            }
        } else {
            comparison
                .push_str("│  📁 Local Files:      None found                              │\n");
            comparison
                .push_str("│                                                                │\n");
            comparison
                .push_str("│  � STATUS: NO LOCAL SAVES                                     │\n");
            comparison
                .push_str("│      ✅ Safe to restore (no files to overwrite)              │\n");
        }
    } else {
        comparison.push_str("Local Files:      Information unavailable\n");
        comparison.push_str("\n🔴 STATUS: LOCAL SAVE INFO UNKNOWN\n");
        comparison.push_str("    Cannot determine local save status\n");
    }

    comparison.push('\n');
    comparison
}

/// Create tags and paths section for snapshot preview
fn create_preview_metadata(snapshot: &Snapshot) -> String {
    let mut metadata = String::new();

    metadata.push_str("🏷️  SNAPSHOT METADATA\n\n");

    // Tags
    if !snapshot.tags.is_empty() {
        let tags_str = snapshot.tags.join(", ");
        let truncated_tags = truncate_string(&tags_str, 60);
        metadata.push_str(&format!("Tags:             {truncated_tags}\n"));
    } else {
        metadata.push_str("Tags:             None\n");
    }

    // Full ID for reference
    metadata.push_str(&format!(
        "Full ID:          {}\n",
        truncate_string(&snapshot.id, 50)
    ));

    // Paths
    if !snapshot.paths.is_empty() {
        metadata.push_str("\nBackup Paths:\n");
        for (i, path) in snapshot.paths.iter().take(5).enumerate() {
            // Limit to 5 paths to prevent overflow
            let truncated_path = truncate_string(path, 70);
            if i == 0 {
                metadata.push_str(&format!("  ├─ {truncated_path}\n"));
            } else if i == snapshot.paths.len() - 1 || i == 4 {
                metadata.push_str(&format!("  └─ {truncated_path}\n"));
            } else {
                metadata.push_str(&format!("  ├─ {truncated_path}\n"));
            }
        }

        // Show count if there are more paths than displayed
        if snapshot.paths.len() > 5 {
            let remaining = snapshot.paths.len() - 5;
            metadata.push_str(&format!("  └─ ... and {remaining} more paths\n"));
        }
    } else {
        metadata.push_str("\nBackup Paths:     None specified\n");
    }

    metadata
}

/// Create rich preview text for snapshot
fn create_snapshot_preview(snapshot: &Snapshot, game_name: &str) -> String {
    let mut preview = String::new();

    // Header
    preview.push_str(&create_preview_header(snapshot, game_name));

    // File statistics
    if let Some(summary) = &snapshot.summary {
        preview.push_str(&create_preview_statistics(summary));
    } else {
        preview.push_str("📊 No detailed statistics available\n");
    }

    // Metadata
    preview.push_str(&create_preview_metadata(snapshot));

    preview
}

/// Create enhanced snapshot preview with local save comparison
fn create_enhanced_snapshot_preview(
    snapshot: &Snapshot,
    game_name: &str,
    local_save_info: &Option<crate::game::utils::save_files::SaveDirectoryInfo>,
) -> String {
    let mut preview = String::new();

    // Header
    preview.push_str(&create_preview_header(snapshot, game_name));

    // Local save comparison section
    preview.push_str(&create_preview_local_comparison(
        local_save_info,
        &snapshot.time,
    ));

    // File statistics
    if let Some(summary) = &snapshot.summary {
        preview.push_str(&create_preview_statistics(summary));
    } else {
        preview.push_str("📊 No detailed statistics available\n");
    }

    // Metadata
    preview.push_str(&create_preview_metadata(snapshot));

    preview
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

/// Select a snapshot interactively for a specific game (cached version)
/// Returns Some(snapshot_id) if a snapshot was selected, None if cancelled
pub fn select_snapshot_interactive(game_name: &str) -> Result<Option<String>> {
    select_snapshot_interactive_with_local_comparison(game_name, None)
}

/// Select a snapshot interactively with optional local save comparison
/// Returns Some(snapshot_id) if a snapshot was selected, None if cancelled
pub fn select_snapshot_interactive_with_local_comparison(
    game_name: &str,
    installation: Option<&crate::game::config::GameInstallation>,
) -> Result<Option<String>> {
    let config = InstantGameConfig::load()
        .context("Failed to load game configuration for snapshot selection")?;

    // Use cached snapshots for better performance
    let snapshots = super::cache::get_snapshots_for_game(game_name, &config)
        .context("Failed to get snapshots for game")?;

    if snapshots.is_empty() {
        println!(
            "❌ No snapshots found for game '{game_name}'.\n\nMake sure backups have been created for this game."
        );
        return Ok(None);
    }

    // Get local save information for comparison if installation is provided
    let local_save_info = if let Some(install) = installation {
        get_save_directory_info(install.save_path.as_path()).ok()
    } else {
        None
    };

    // Show selection prompt
    // Create wrapper snapshots that include local comparison info
    let enhanced_snapshots: Vec<EnhancedSnapshot> = snapshots
        .into_iter()
        .map(|snapshot| EnhancedSnapshot {
            snapshot,
            local_save_info: local_save_info.clone(),
            game_name: game_name.to_string(),
        })
        .collect();

    let selected = FzfWrapper::select_one(enhanced_snapshots)
        .map_err(|e| anyhow::anyhow!("Failed to select snapshot: {}", e))?;

    match selected {
        Some(enhanced) => Ok(Some(enhanced.snapshot.id)),
        None => {
            println!("No snapshot selected.");
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
                        Ok(TimeComparison::LocalNewer) => " ⚠️LOCAL NEWER",
                        Ok(TimeComparison::SnapshotNewer) => " ✓SNAPSHOT NEWER",
                        Ok(TimeComparison::Same) => " =SAME TIME",
                        Ok(TimeComparison::Error(_)) => " ⚠️COMPARE ERROR",
                        Err(_) => " ⚠️COMPARE ERROR",
                    }
                } else {
                    " ⚠️NO LOCAL TIME"
                }
            } else {
                " 📁NO LOCAL SAVES"
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
        let preview_text = create_enhanced_snapshot_preview(
            &self.snapshot,
            &self.game_name,
            &self.local_save_info,
        );
        crate::menu::protocol::FzfPreview::Text(preview_text)
    }
}
