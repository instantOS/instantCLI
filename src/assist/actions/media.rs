use anyhow::{Context, Result};
use std::process::Command;

pub fn music() -> Result<()> {
    Command::new("playerctl")
        .arg("play-pause")
        .spawn()
        .context("Failed to control playback with playerctl")?;
    Ok(())
}

pub fn previous_track() -> Result<()> {
    Command::new("playerctl")
        .arg("previous")
        .spawn()
        .context("Failed to go to previous track with playerctl")?;
    Ok(())
}

pub fn next_track() -> Result<()> {
    Command::new("playerctl")
        .arg("next")
        .spawn()
        .context("Failed to go to next track with playerctl")?;
    Ok(())
}

pub fn control_media() -> Result<()> {
    use crate::menu::client::MenuClient;
    use crate::menu::protocol::{FzfPreview, SerializableMenuItem};
    use std::collections::HashMap;

    // Get list of players
    let output = Command::new("playerctl")
        .arg("-l")
        .output()
        .context("Failed to list players")?;

    let players_str = String::from_utf8_lossy(&output.stdout);
    let players: Vec<&str> = players_str.lines().collect();

    if players.is_empty() {
        println!("No media players found.");
        return Ok(());
    }

    let mut items = Vec::new();

    for player in players {
        // Get status
        let status_output = Command::new("playerctl")
            .arg("-p")
            .arg(player)
            .arg("status")
            .output();

        let status = match status_output {
            Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_string(),
            Err(_) => "Unknown".to_string(),
        };

        // Get metadata
        let metadata_output = Command::new("playerctl")
            .arg("-p")
            .arg(player)
            .arg("metadata")
            .arg("--format")
            .arg("{{ artist }} - {{ title }}")
            .output();

        let track_info = match metadata_output {
            Ok(out) => {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if s.is_empty() {
                    "No track info".to_string()
                } else {
                    s
                }
            }
            Err(_) => "No track info".to_string(),
        };

        let icon = match status.as_str() {
            "Playing" => "▶",
            "Paused" => "⏸",
            "Stopped" => "⏹",
            _ => "?",
        };

        let display_text = format!("{} {}: {}", icon, player, track_info);

        let mut metadata_map = HashMap::new();
        metadata_map.insert("player".to_string(), player.to_string());

        items.push(SerializableMenuItem {
            display_text,
            preview: FzfPreview::None,
            metadata: Some(metadata_map),
        });
    }

    let client = MenuClient::new();
    let selected = client.choice(
        "Select media player to toggle play/pause:".to_string(),
        items,
        false,
    )?;

    if let Some(item) = selected.first() {
        if let Some(metadata) = &item.metadata {
            if let Some(player) = metadata.get("player") {
                Command::new("playerctl")
                    .arg("-p")
                    .arg(player)
                    .arg("play-pause")
                    .spawn()
                    .context("Failed to toggle playback")?;
            }
        }
    }

    Ok(())
}
