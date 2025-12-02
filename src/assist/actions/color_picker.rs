use anyhow::{Context, Result};
use std::process::Command;

use crate::assist::utils::{copy_to_clipboard, show_notification, show_notification_with_icon};
use crate::common::display_server::DisplayServer;

pub fn pick_color() -> Result<()> {
    let display_server = DisplayServer::detect();

    if display_server.is_wayland() {
        // Wayland: hyprpicker
        let output = Command::new("hyprpicker")
            .output()
            .context("Failed to run hyprpicker")?;

        if !output.status.success() {
            // If user cancelled (exit code 1 usually), just return Ok
            return Ok(());
        }

        let color = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if color.is_empty() {
            return Ok(());
        }

        copy_to_clipboard(color.as_bytes(), &display_server)?;

        // Optional: notify user
        show_notification("Color copied", &color)?;
    } else if display_server.is_x11() {
        // X11: xcolor
        let output = Command::new("xcolor")
            .output()
            .context("Failed to run xcolor")?;

        if !output.status.success() {
            // If user cancelled, just return Ok
            return Ok(());
        }

        let color = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if color.is_empty() {
            return Ok(());
        }

        copy_to_clipboard(color.as_bytes(), &display_server)?;

        // Create preview image for notification
        let temp_dir = std::env::temp_dir();
        let icon_path = temp_dir.join(format!("color_{}.png", chrono::Local::now().timestamp()));

        let convert_status = Command::new("convert")
            .args(["-size", "45x45", &format!("xc:{}", color)])
            .arg(&icon_path)
            .status()
            .context("Failed to create preview image")?;

        if convert_status.success() {
            show_notification_with_icon(
                &format!("{} copied to clipboard", color),
                "",
                icon_path.to_str().unwrap_or(""),
            )?;

            // Clean up the temp file after a short delay to ensure notification server reads it
            // We spawn a thread to avoid blocking the main thread
            let path_clone = icon_path.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(2));
                let _ = std::fs::remove_file(path_clone);
            });
        } else {
            show_notification(&format!("{} copied to clipboard", color), "")?;
        }
    } else {
        anyhow::bail!("Unknown display server - cannot pick color");
    }

    Ok(())
}
