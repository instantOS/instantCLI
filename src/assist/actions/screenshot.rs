use anyhow::{Context, Result};
use std::io::Write;
use std::process::Command;

use crate::assist::utils::AreaSelectionConfig;
use crate::common::display_server::DisplayServer;
use crate::common::paths;

pub fn screenshot_annotate() -> Result<()> {
    use crate::common::display_server::DisplayServer;

    let display_server = DisplayServer::detect();

    if display_server.is_wayland() {
        let flameshot_running = Command::new("pgrep")
            .arg("flameshot")
            .output()
            .map(|o| !o.stdout.is_empty())
            .unwrap_or(false);

        if !flameshot_running {
            Command::new("flameshot")
                .env("SDL_VIDEODRIVER", "wayland")
                .env("_JAVA_AWT_WM_NONREPARENTING", "1")
                .env("QT_QPA_PLATFORM", "wayland")
                .env("XDG_CURRENT_DESKTOP", "sway")
                .env("XDG_SESSION_DESKTOP", "sway")
                .spawn()
                .context("Failed to start flameshot daemon")?;

            std::thread::sleep(std::time::Duration::from_secs(2));
        }

        Command::new("flameshot")
            .arg("gui")
            .env("SDL_VIDEODRIVER", "wayland")
            .env("_JAVA_AWT_WM_NONREPARENTING", "1")
            .env("QT_QPA_PLATFORM", "wayland")
            .env("XDG_CURRENT_DESKTOP", "sway")
            .env("XDG_SESSION_DESKTOP", "sway")
            .spawn()
            .context("Failed to launch flameshot gui")?;
    } else {
        std::thread::sleep(std::time::Duration::from_millis(100));

        Command::new("flameshot")
            .arg("gui")
            .spawn()
            .context("Failed to launch flameshot gui")?;
    }

    Ok(())
}

pub fn screenshot_to_clipboard() -> Result<()> {
    // Create cached configuration to avoid repeated display server detection
    let config = AreaSelectionConfig::new();

    // Get area selection using the cached configuration
    let geometry = match config.select_area() {
        Ok(geom) => geom,
        Err(_) => {
            // Area selection was cancelled or failed - just return success without taking screenshot
            return Ok(());
        }
    };

    let display_server = config.display_server();

    if display_server.is_wayland() {
        // Capture screenshot with grim
        let grim_output = Command::new("grim")
            .args(["-g", &geometry, "-"])
            .output()
            .context("Failed to capture screenshot with grim")?;

        if !grim_output.status.success() {
            anyhow::bail!("Failed to capture screenshot");
        }

        // Copy to clipboard with wl-copy
        let mut wl_copy = Command::new("wl-copy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .context("Failed to start wl-copy")?;

        if let Some(mut stdin) = wl_copy.stdin.take() {
            stdin
                .write_all(&grim_output.stdout)
                .context("Failed to write screenshot to wl-copy")?;
        }

        wl_copy.wait().context("Failed to wait for wl-copy")?;
    } else if display_server.is_x11() {
        // Capture screenshot with import (ImageMagick)
        let import_output = Command::new("import")
            .args(["-window", "root", "-crop", &geometry, "png:-"])
            .output()
            .context("Failed to capture screenshot with import")?;

        if !import_output.status.success() {
            anyhow::bail!("Failed to capture screenshot");
        }

        // Copy to clipboard with xclip
        let mut xclip = Command::new("xclip")
            .args(["-selection", "clipboard", "-t", "image/png"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .context("Failed to start xclip")?;

        if let Some(mut stdin) = xclip.stdin.take() {
            stdin
                .write_all(&import_output.stdout)
                .context("Failed to write screenshot to xclip")?;
        }

        xclip.wait().context("Failed to wait for xclip")?;
    } else {
        anyhow::bail!("Unknown display server - cannot take screenshot");
    }

    Ok(())
}

pub fn screenshot_to_imgur() -> Result<()> {
    // Create cached configuration to avoid repeated display server detection
    let config = AreaSelectionConfig::new();

    // Get area selection using the cached configuration
    let geometry = match config.select_area() {
        Ok(geom) => geom,
        Err(_) => {
            // Area selection was cancelled or failed - just return success without taking screenshot
            return Ok(());
        }
    };

    let display_server = config.display_server();

    let screenshot_data = if display_server.is_wayland() {
        // Capture screenshot with grim
        let grim_output = Command::new("grim")
            .args(["-g", &geometry, "-"])
            .output()
            .context("Failed to capture screenshot with grim")?;

        if !grim_output.status.success() {
            anyhow::bail!("Failed to capture screenshot");
        }

        grim_output.stdout
    } else if display_server.is_x11() {
        // Capture screenshot with import (ImageMagick)
        let import_output = Command::new("import")
            .args(["-window", "root", "-crop", &geometry, "png:-"])
            .output()
            .context("Failed to capture screenshot with import")?;

        if !import_output.status.success() {
            anyhow::bail!("Failed to capture screenshot");
        }

        import_output.stdout
    } else {
        anyhow::bail!("Unknown display server - cannot take screenshot");
    };

    // Upload to Imgur
    let imgur_link =
        upload_to_imgur(&screenshot_data).context("Failed to upload screenshot to Imgur")?;

    // Copy link to clipboard
    if display_server.is_wayland() {
        let mut wl_copy = Command::new("wl-copy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .context("Failed to start wl-copy")?;

        if let Some(mut stdin) = wl_copy.stdin.take() {
            stdin
                .write_all(imgur_link.as_bytes())
                .context("Failed to write Imgur link to wl-copy")?;
        }

        wl_copy.wait().context("Failed to wait for wl-copy")?;
    } else if display_server.is_x11() {
        let mut xclip = Command::new("xclip")
            .args(["-selection", "clipboard"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .context("Failed to start xclip")?;

        if let Some(mut stdin) = xclip.stdin.take() {
            stdin
                .write_all(imgur_link.as_bytes())
                .context("Failed to write Imgur link to xclip")?;
        }

        xclip.wait().context("Failed to wait for xclip")?;
    }

    // Show notification
    Command::new("notify-send")
        .args(["Imgur link copied", &imgur_link])
        .spawn()
        .context("Failed to show notification")?;

    Ok(())
}

fn upload_to_imgur(image_data: &[u8]) -> Result<String> {
    let image_data = image_data.to_vec();

    // Use spawn_blocking to avoid runtime nesting issues
    std::thread::spawn(move || {
        let rt =
            tokio::runtime::Runtime::new().context("Failed to create tokio runtime for upload")?;
        rt.block_on(upload_to_imgur_async(&image_data))
    })
    .join()
    .map_err(|_| anyhow::anyhow!("Thread panicked during upload"))?
}

async fn upload_to_imgur_async(image_data: &[u8]) -> Result<String> {
    use std::time::Duration;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("Failed to create HTTP client")?;

    let form = reqwest::multipart::Form::new().part(
        "image",
        reqwest::multipart::Part::bytes(image_data.to_vec())
            .file_name("screenshot.png")
            .mime_str("image/png")?,
    );

    let response = client
        .post("https://api.imgur.com/3/image")
        .header("Authorization", "Client-ID 546c25a59c58ad7")
        .multipart(form)
        .send()
        .await
        .context("Failed to send upload request to Imgur")?;

    if !response.status().is_success() {
        anyhow::bail!("Imgur API returned error: {}", response.status());
    }

    let json: serde_json::Value = response
        .json()
        .await
        .context("Failed to parse Imgur response as JSON")?;

    let link = json["data"]["link"]
        .as_str()
        .context("Failed to extract link from Imgur response")?
        .to_string();

    if link.is_empty() {
        anyhow::bail!("Imgur returned empty link");
    }

    Ok(link)
}

pub fn fullscreen_screenshot() -> Result<()> {
    let display_server = DisplayServer::detect();

    // Generate filename with timestamp
    let timestamp = chrono::Local::now().format("%Y%m%d%H%M%S");
    let filename = format!("{}.png", timestamp);

    // Get pictures directory
    let pictures_dir = paths::pictures_dir().context("Failed to determine pictures directory")?;
    let output_path = pictures_dir.join(&filename);

    if display_server.is_wayland() {
        // Use grim for Wayland
        let status = Command::new("grim")
            .arg(output_path.to_str().context("Invalid path encoding")?)
            .status()
            .context("Failed to execute grim")?;

        if !status.success() {
            anyhow::bail!("Failed to capture fullscreen screenshot with grim");
        }
    } else if display_server.is_x11() {
        // Check if picom is running and add small delay
        let picom_running = Command::new("pgrep")
            .arg("picom")
            .output()
            .map(|o| !o.stdout.is_empty())
            .unwrap_or(false);

        if picom_running {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }

        // Add small delay for stability
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Use ImageMagick's import for X11
        let status = Command::new("import")
            .args(["-window", "root"])
            .arg(output_path.to_str().context("Invalid path encoding")?)
            .status()
            .context("Failed to execute import")?;

        if !status.success() {
            anyhow::bail!("Failed to capture fullscreen screenshot with import");
        }
    } else {
        anyhow::bail!("Unknown display server - cannot take fullscreen screenshot");
    }

    Ok(())
}
