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
    use std::collections::HashMap;

    // Get list of players
    let output = Command::new("playerctl")
        .arg("-l")
        .output()
        .context("Failed to list players")?;

    let players_str = String::from_utf8_lossy(&output.stdout);
    let players: Vec<&str> = players_str.lines().collect();

    if players.is_empty() {
        return Ok(());
    }

    let mut chords = Vec::new();
    let mut player_map = HashMap::new();

    // Keys to use for players: 1-9, then a-z
    let keys = "123456789abcdefghijklmnopqrstuvwxyz";

    for (i, player) in players.iter().enumerate() {
        if i >= keys.len() {
            break;
        }
        let key = keys.chars().nth(i).unwrap();

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

        let description = format!("{} {}: {}", icon, player, track_info);
        chords.push(format!("{}:{}", key, description));
        player_map.insert(key.to_string(), player.to_string());
    }

    let client = MenuClient::new();
    let selected_key = client.chord(chords)?;

    if let Some(key) = selected_key {
        if let Some(player) = player_map.get(&key) {
            Command::new("playerctl")
                .arg("-p")
                .arg(player)
                .arg("play-pause")
                .spawn()
                .context("Failed to toggle playback")?;
        }
    }

    Ok(())
}
