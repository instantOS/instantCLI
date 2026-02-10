use anyhow::Result;
use std::process::Command;

use crate::assist::utils;

pub fn play_bruh_sound() -> Result<()> {
    // Create cache directory if it doesn't exist
    let cache_dir = dirs::cache_dir().unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    let assist_cache_dir = cache_dir.join("instantassist");
    std::fs::create_dir_all(&assist_cache_dir)?;

    // Define the bruh sound file path
    let bruh_file = assist_cache_dir.join("bruh.m4a");

    // Download the bruh sound if it doesn't exist
    if !bruh_file.exists() {
        // Show notification about downloading
        if Command::new("notify-send")
            .arg("Downloading bruh sound")
            .status()
            .is_err()
        {
            println!("Downloading bruh sound...");
        }

        // Download the file
        let url = "http://bruhsound.surge.sh/bruh.m4a";
        let response = reqwest::blocking::get(url)?;
        let content = response.bytes()?;
        std::fs::write(&bruh_file, content)?;
    }

    // Check if the file exists
    if !bruh_file.exists() {
        eprintln!("Failed to download or find bruh sound file");
        return Ok(());
    }

    // Play the bruh sound with default config (ignore user config and resume position)
    Command::new("mpv")
        .arg("--no-config")
        .arg("--no-resume-playback")
        .arg(&bruh_file)
        .status()?;

    Ok(())
}

pub fn asciiquarium() -> Result<()> {
    utils::launch_in_terminal("asciiquarium")
}

pub fn cmatrix() -> Result<()> {
    utils::launch_in_terminal("cmatrix")
}
