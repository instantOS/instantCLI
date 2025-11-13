use anyhow::{Context, Result};
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
    use crate::assist::utils::{capture_area_to_memory, copy_image_to_clipboard};

    let config = AreaSelectionConfig::new();

    let geometry = match config.select_area() {
        Ok(geom) => geom,
        Err(_) => return Ok(()),
    };

    let display_server = config.display_server();
    let screenshot_data = capture_area_to_memory(&geometry, display_server)?;

    copy_image_to_clipboard(&screenshot_data, "image/png", display_server)?;

    Ok(())
}

pub fn screenshot_to_imgur() -> Result<()> {
    use crate::assist::utils::{capture_area_to_memory, copy_to_clipboard};

    let config = AreaSelectionConfig::new();

    let geometry = match config.select_area() {
        Ok(geom) => geom,
        Err(_) => return Ok(()),
    };

    let display_server = config.display_server();
    let screenshot_data = capture_area_to_memory(&geometry, display_server)?;

    let imgur_link =
        upload_to_imgur(&screenshot_data).context("Failed to upload screenshot to Imgur")?;

    // Copy link to clipboard
    copy_to_clipboard(imgur_link.as_bytes(), display_server)?;

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

pub fn screenshot_ocr() -> Result<()> {
    use crate::assist::utils::{capture_area_to_file, copy_to_clipboard};

    let config = AreaSelectionConfig::new();

    let geometry = match config.select_area() {
        Ok(geom) => geom,
        Err(_) => return Ok(()),
    };

    let display_server = config.display_server();

    let pictures_dir = paths::pictures_dir().context("Failed to determine pictures directory")?;
    let image_path = pictures_dir.join("ocr.png");

    capture_area_to_file(&geometry, &image_path, display_server)?;

    // Run tesseract OCR on the captured image
    let ocr_output = Command::new("tesseract")
        .arg(&image_path)
        .arg("stdout")
        .output()
        .context("Failed to run tesseract OCR")?;

    if !ocr_output.status.success() {
        anyhow::bail!("Tesseract OCR failed");
    }

    // Get detected text and clean it up (remove form feed character)
    let detected_text = String::from_utf8_lossy(&ocr_output.stdout)
        .trim()
        .replace('\x0c', "")
        .to_string();

    if detected_text.is_empty() {
        Command::new("notify-send")
            .args(["-a", "instantASSIST", "No text detected"])
            .spawn()
            .context("Failed to show notification")?;
        return Ok(());
    }

    // Copy detected text to clipboard
    copy_to_clipboard(detected_text.as_bytes(), display_server)?;

    // Show notification with detected text
    Command::new("notify-send")
        .args(["-a", "instantASSIST", "Detected text", &detected_text])
        .spawn()
        .context("Failed to show notification")?;

    Ok(())
}
