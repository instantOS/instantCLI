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

        format!("{} - {} ({})", game_name, date, host)
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

        format!("{}\n({})", formatted_date, time_ago)
    } else {
        iso_date.to_string()
    }
}

/// Create header section for snapshot preview
fn create_preview_header(snapshot: &Snapshot, game_name: &str) -> String {
    format!(
        "🎮 Game: {}\n🖥️  Host: {}\n📅 Date: {}\n🏷️  ID: {}\n🆔 Full ID: {}\n\n",
        game_name,
        snapshot.hostname,
        format_date_with_time_ago(&snapshot.time),
        snapshot.short_id,
        snapshot.id
    )
}

/// Create file statistics section for snapshot preview
fn create_preview_statistics(summary: &crate::restic::wrapper::SnapshotSummary) -> String {
    let mut stats = String::new();
    let total_files = summary.files_new + summary.files_changed + summary.files_unmodified;

    stats.push_str("📊 File Statistics:\n");
    stats.push_str(&format!("  • Total files: {}\n", total_files));

    if summary.files_new > 0 {
        stats.push_str(&format!("  • New files: {}\n", summary.files_new));
    }
    if summary.files_changed > 0 {
        stats.push_str(&format!("  • Changed files: {}\n", summary.files_changed));
    }
    if summary.files_unmodified > 0 {
        stats.push_str(&format!(
            "  • Unmodified files: {}\n",
            summary.files_unmodified
        ));
    }

    // Data size
    if summary.data_added > 0 {
        let size_mb = summary.data_added as f64 / 1_048_576.0;
        stats.push_str(&format!("  • Data added: {:.2} MB\n", size_mb));
    }

    // Duration
    if let (Ok(start), Ok(end)) = (
        chrono::DateTime::parse_from_rfc3339(&summary.backup_start),
        chrono::DateTime::parse_from_rfc3339(&summary.backup_end),
    ) {
        let duration = end.signed_duration_since(start);
        let duration_secs = duration.num_seconds();
        if duration_secs > 0 {
            stats.push_str(&format!("  • Backup duration: {} seconds\n", duration_secs));
        }
    }

    stats
}

/// Create local save comparison section for snapshot preview
fn create_preview_local_comparison(
    local_save_info: &Option<crate::game::utils::save_files::SaveDirectoryInfo>,
    snapshot_time: &str,
) -> String {
    let mut comparison = String::new();

    comparison.push_str("💾 Local Save Status:\n");

    if let Some(local_info) = local_save_info {
        if local_info.file_count > 0 {
            comparison.push_str(&format!(
                "  • Files: {} ({})\n",
                local_info.file_count,
                format_file_size(local_info.total_size)
            ));

            if let Some(local_time) = local_info.last_modified {
                let local_time_str = format_system_time_for_display(Some(local_time));
                comparison.push_str(&format!("  • Last modified: {}\n", local_time_str));

                // Add comparison result
                match compare_snapshot_vs_local(snapshot_time, local_time) {
                    Ok(TimeComparison::LocalNewer) => {
                        comparison.push_str("  • Status: ⚠️  LOCAL SAVES ARE NEWER\n");
                        comparison
                            .push_str("  • ⚠️  Restoring would overwrite newer local saves\n");
                    }
                    Ok(TimeComparison::SnapshotNewer) => {
                        comparison.push_str("  • Status: ✓ SNAPSHOT IS NEWER\n");
                        comparison.push_str("  • ✓ Safe to restore (newer backup)\n");
                    }
                    Ok(TimeComparison::Same) => {
                        comparison.push_str("  • Status: = TIMES MATCH\n");
                        comparison.push_str("  • ✓ Local saves match backup timestamp\n");
                    }
                    Ok(TimeComparison::Error(msg)) => {
                        comparison
                            .push_str(&format!("  • Status: ⚠️  COMPARISON ERROR: {}\n", msg));
                    }
                    Err(_) => {
                        comparison.push_str("  • Status: ⚠️  COULDN'T COMPARE TIMES\n");
                    }
                }
            } else {
                comparison.push_str("  • Status: ⚠️  MODIFICATION TIME UNKNOWN\n");
            }
        } else {
            comparison.push_str("  • Status: 📁 NO LOCAL SAVES FOUND\n");
            comparison.push_str("  • ✓ Safe to restore (no local files to overwrite)\n");
        }
    } else {
        comparison.push_str("  • Status: ❓ LOCAL SAVE INFO UNKNOWN\n");
    }

    comparison.push('\n');
    comparison
}

/// Create tags and paths section for snapshot preview
fn create_preview_metadata(snapshot: &Snapshot) -> String {
    let mut metadata = String::new();

    // Tags
    if !snapshot.tags.is_empty() {
        metadata.push_str("🏷️  Tags: ");
        metadata.push_str(&snapshot.tags.join(", "));
        metadata.push('\n');
    }

    // Paths
    if !snapshot.paths.is_empty() {
        metadata.push_str("\n📁 Backup Paths:\n");
        for path in &snapshot.paths {
            metadata.push_str(&format!("  • {}\n", path));
        }
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
            "❌ No snapshots found for game '{}'.\n\nMake sure backups have been created for this game.",
            game_name
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
            //TODO: replace with print
            FzfWrapper::message("No snapshot selected.")
                .context("Failed to show no selection message")?;
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

        format!("{} ({}){}", date, host, comparison_indicator)
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
