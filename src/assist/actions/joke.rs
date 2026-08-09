use anyhow::Result;
use std::process::Command;

use crate::assist::utils;

pub fn bruh() -> Result<()> {
    // Create cache directory if it doesn't exist. Honor $TMPDIR via
    // std::env::temp_dir() so platforms like Termux (where /tmp doesn't exist
    // and $PREFIX/tmp is used instead) still resolve to a writable path.
    let cache_dir = dirs::cache_dir().unwrap_or_else(std::env::temp_dir);
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

        // Download the file on a dedicated OS thread.
        //
        // `reqwest::blocking` creates its own runtime, which cannot be created
        // or dropped inside an async context (ins runs under #[tokio::main] and
        // panics with "Cannot drop a runtime in a context where blocking is not
        // allowed"). A fresh OS thread has no runtime context, so the blocking
        // client is safe there.
        let url = "http://bruhsound.surge.sh/bruh.m4a";
        let content = std::thread::spawn(move || -> Result<Vec<u8>> {
            let response = reqwest::blocking::get(url)?;
            Ok(response.bytes()?.to_vec())
        })
        .join()
        .map_err(|_| anyhow::anyhow!("Bruh sound download thread panicked"))??;
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
