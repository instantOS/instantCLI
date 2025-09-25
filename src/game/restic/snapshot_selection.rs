use crate::fzf_wrapper::{FzfSelectable, FzfWrapper};
use crate::restic::wrapper::Snapshot;
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

/// Select a snapshot interactively for a specific game
/// Returns Some(snapshot_id) if a snapshot was selected, None if cancelled
pub fn select_snapshot_interactive(game_name: &str) -> Result<Option<String>> {
    // Import here to avoid circular dependencies
    use crate::game::config::InstantGameConfig;

    let config = InstantGameConfig::load()
        .context("Failed to load game configuration for snapshot selection")?;

    // Get snapshots for this game
    let restic = crate::restic::wrapper::ResticWrapper::new(
        config.repo.as_path().to_string_lossy().to_string(),
        config.repo_password.clone(),
    );

    let snapshots = restic.list_snapshots_filtered(Some(vec![
        "instantgame".to_string(),
        game_name.to_string(),
    ])).context("Failed to list snapshots for game")?;

    // Parse JSON snapshots
    let mut parsed_snapshots: Vec<Snapshot> = serde_json::from_str(&snapshots)
        .context("Failed to parse snapshot data")?;

    // Sort by date (newest first)
    parsed_snapshots.sort_by(|a, b| b.time.cmp(&a.time));

    if parsed_snapshots.is_empty() {
        FzfWrapper::message(&format!(
            "❌ No snapshots found for game '{}'.\n\nMake sure backups have been created for this game.",
            game_name
        )).context("Failed to show no snapshots message")?;
        return Ok(None);
    }

    // Show selection prompt
    FzfWrapper::message(&format!(
        "Select a snapshot to restore for game '{}':",
        game_name
    )).context("Failed to show snapshot selection prompt")?;

    let selected = FzfWrapper::select_one(parsed_snapshots)
        .map_err(|e| anyhow::anyhow!("Failed to select snapshot: {}", e))?;

    match selected {
        Some(snapshot) => Ok(Some(snapshot.id)),
        None => {
            FzfWrapper::message("No snapshot selected.")
                .context("Failed to show no selection message")?;
            Ok(None)
        }
    }
}