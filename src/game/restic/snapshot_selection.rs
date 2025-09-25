use crate::fzf_wrapper::{FzfSelectable, FzfWrapper};
use crate::restic::wrapper::Snapshot;
use crate::game::config::InstantGameConfig;
use crate::game::utils::save_files::{get_save_directory_info, compare_snapshot_vs_local, TimeComparison, format_system_time_for_display};
use anyhow::{Context, Result};

impl FzfSelectable for Snapshot {
    fn fzf_display_text(&self) -> String {
        let date = format_date(&self.time);
        let host = &self.hostname;

        // Extract game name from tags if available
        let game_name = self.tags
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
        let game_name = self.tags
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

/// Create rich preview text for snapshot
fn create_snapshot_preview(snapshot: &Snapshot, game_name: &str) -> String {
    let mut preview = String::new();

    // Header
    preview.push_str(&format!("🎮 Game: {}\n", game_name));
    preview.push_str(&format!("🖥️  Host: {}\n", snapshot.hostname));
    preview.push_str(&format!("📅 Date: {}\n", format_date(&snapshot.time)));
    preview.push_str(&format!("🏷️  ID: {}\n\n", snapshot.short_id));

    // File statistics
    if let Some(summary) = &snapshot.summary {
        let total_files = summary.files_new + summary.files_changed + summary.files_unmodified;
        preview.push_str("📊 File Statistics:\n");
        preview.push_str(&format!("  • Total files: {}\n", total_files));
        if summary.files_new > 0 {
            preview.push_str(&format!("  • New files: {}\n", summary.files_new));
        }
        if summary.files_changed > 0 {
            preview.push_str(&format!("  • Changed files: {}\n", summary.files_changed));
        }
        if summary.files_unmodified > 0 {
            preview.push_str(&format!("  • Unmodified files: {}\n", summary.files_unmodified));
        }

        // Data size
        if summary.data_added > 0 {
            let size_mb = summary.data_added as f64 / 1_048_576.0;
            preview.push_str(&format!("  • Data added: {:.2} MB\n", size_mb));
        }

        // Duration
        if let (Ok(start), Ok(end)) = (
            chrono::DateTime::parse_from_rfc3339(&summary.backup_start),
            chrono::DateTime::parse_from_rfc3339(&summary.backup_end),
        ) {
            let duration = end.signed_duration_since(start);
            let duration_secs = duration.num_seconds();
            if duration_secs > 0 {
                preview.push_str(&format!("  • Backup duration: {} seconds\n", duration_secs));
            }
        }
    } else {
        preview.push_str("📊 No detailed statistics available\n");
    }

    // Tags
    if !snapshot.tags.is_empty() {
        preview.push_str("\n🏷️  Tags: ");
        preview.push_str(&snapshot.tags.join(", "));
        preview.push('\n');
    }

    // Paths
    if !snapshot.paths.is_empty() {
        preview.push_str("\n📁 Paths:\n");
        for path in &snapshot.paths {
            preview.push_str(&format!("  • {}\n", path));
        }
    }

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
    preview.push_str(&format!("🎮 Game: {}\n", game_name));
    preview.push_str(&format!("🖥️  Host: {}\n", snapshot.hostname));
    preview.push_str(&format!("📅 Date: {}\n", format_date(&snapshot.time)));
    preview.push_str(&format!("🏷️  ID: {}\n\n", snapshot.short_id));

    // Local save comparison section
    if let Some(local_info) = local_save_info {
        preview.push_str("💾 Local Save Status:\n");

        if local_info.file_count > 0 {
            preview.push_str(&format!("  • Files: {} ({})\n", local_info.file_count, format_file_size(local_info.total_size)));

            if let Some(local_time) = local_info.last_modified {
                let local_time_str = format_system_time_for_display(Some(local_time));
                preview.push_str(&format!("  • Last modified: {}\n", local_time_str));

                // Add comparison result
                match compare_snapshot_vs_local(&snapshot.time, local_time) {
                    Ok(TimeComparison::LocalNewer) => {
                        preview.push_str("  • Status: ⚠️ LOCAL SAVES ARE NEWER\n");
                    }
                    Ok(TimeComparison::SnapshotNewer) => {
                        preview.push_str("  • Status: ✓ SNAPSHOT IS NEWER\n");
                    }
                    Ok(TimeComparison::Same) => {
                        preview.push_str("  • Status: = TIMES MATCH\n");
                    }
                    Ok(TimeComparison::Error(msg)) => {
                        preview.push_str(&format!("  • Status: ⚠️ COMPARISON ERROR: {}\n", msg));
                    }
                    Err(_) => {
                        preview.push_str("  • Status: ⚠️ COULDN'T COMPARE TIMES\n");
                    }
                }
            } else {
                preview.push_str("  • Status: ⚠️ MODIFICATION TIME UNKNOWN\n");
            }
        } else {
            preview.push_str("  • Status: 📁 NO LOCAL SAVES FOUND\n");
        }
        preview.push('\n');
    } else {
        preview.push_str("💾 Local Save Status: UNKNOWN\n\n");
    }

    // File statistics
    if let Some(summary) = &snapshot.summary {
        let total_files = summary.files_new + summary.files_changed + summary.files_unmodified;
        preview.push_str("📊 File Statistics:\n");
        preview.push_str(&format!("  • Total files: {}\n", total_files));
        if summary.files_new > 0 {
            preview.push_str(&format!("  • New files: {}\n", summary.files_new));
        }
        if summary.files_changed > 0 {
            preview.push_str(&format!("  • Changed files: {}\n", summary.files_changed));
        }
        if summary.files_unmodified > 0 {
            preview.push_str(&format!("  • Unmodified files: {}\n", summary.files_unmodified));
        }

        // Data size
        if summary.data_added > 0 {
            let size_mb = summary.data_added as f64 / 1_048_576.0;
            preview.push_str(&format!("  • Data added: {:.2} MB\n", size_mb));
        }

        // Duration
        if let (Ok(start), Ok(end)) = (
            chrono::DateTime::parse_from_rfc3339(&summary.backup_start),
            chrono::DateTime::parse_from_rfc3339(&summary.backup_end),
        ) {
            let duration = end.signed_duration_since(start);
            let duration_secs = duration.num_seconds();
            if duration_secs > 0 {
                preview.push_str(&format!("  • Backup duration: {} seconds\n", duration_secs));
            }
        }
    } else {
        preview.push_str("📊 No detailed statistics available\n");
    }

    // Tags
    if !snapshot.tags.is_empty() {
        preview.push_str("\n🏷️  Tags: ");
        preview.push_str(&snapshot.tags.join(", "));
        preview.push('\n');
    }

    // Paths
    if !snapshot.paths.is_empty() {
        preview.push_str("\n📁 Paths:\n");
        for path in &snapshot.paths {
            preview.push_str(&format!("  • {}\n", path));
        }
    }

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
        FzfWrapper::message(&format!(
            "❌ No snapshots found for game '{}'.\n\nMake sure backups have been created for this game.",
            game_name
        )).context("Failed to show no snapshots message")?;
        return Ok(None);
    }

    // Get local save information for comparison if installation is provided
    let local_save_info = if let Some(install) = installation {
        get_save_directory_info(install.save_path.as_path()).ok()
    } else {
        None
    };

    // Show selection prompt
    FzfWrapper::message(&format!(
        "Select a snapshot to restore for game '{}':",
        game_name
    )).context("Failed to show snapshot selection prompt")?;

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
        let game_name = &self.game_name;

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

        format!("{} - {} ({}){}", game_name, date, host, comparison_indicator)
    }

    fn fzf_key(&self) -> String {
        self.snapshot.id.clone()
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