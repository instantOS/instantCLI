use anyhow::{Context, Result};
use reqwest::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use reqwest::multipart::{Form, Part};
use serde_json::json;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::time::sleep;

use super::cli::AuphonicArgs;
use super::config::{VideoConfig, VideoDirectories};
use super::utils::{canonicalize_existing, compute_file_hash};
use crate::ui::prelude::{Level, emit};

const BASE_URL: &str = "https://auphonic.com/api";

pub async fn handle_auphonic(args: AuphonicArgs) -> Result<()> {
    let input_path = canonicalize_existing(&args.input_file)?;
    let input_hash = compute_file_hash(&input_path)?;

    let directories = VideoDirectories::new()?;
    let project_paths = directories.project_paths(&input_hash);
    project_paths.ensure_directories()?;

    let cache_file_name = format!("{}_auphonic.mp3", input_hash);
    let cache_path = project_paths.transcript_dir().join(&cache_file_name);

    if cache_path.exists() && !args.force {
        emit(
            Level::Info,
            "video.auphonic.cached",
            &format!("Auphonic result already cached at {}", cache_path.display()),
            None,
        );
        copy_to_output(&cache_path, &input_path)?;
        return Ok(());
    }

    // Load config
    let config = VideoConfig::load()?;
    let api_key = args
        .api_key
        .or(config.auphonic_api_key)
        .context("Auphonic API key not found. Please provide it via --api-key or in ~/.config/instant/video.toml")?;

    let client = Client::new();

    // Get or create preset
    let preset_uuid = if let Some(uuid) = args.preset.or(config.auphonic_preset_uuid) {
        uuid
    } else {
        create_or_get_preset(&client, &api_key).await?
    };

    // Start production
    let production_uuid = start_production(&client, &api_key, &preset_uuid, &input_path).await?;

    // Poll status
    wait_for_production(&client, &api_key, &production_uuid).await?;

    // Download result
    download_result(&client, &api_key, &production_uuid, &cache_path).await?;

    // Check for jingles and trim if necessary
    let original_duration = get_duration(&input_path).context("Failed to get original duration")?;
    let processed_duration =
        get_duration(&cache_path).context("Failed to get processed duration")?;

    if processed_duration > original_duration {
        let diff = processed_duration - original_duration;
        // If difference is significant (e.g. > 1 second), assume jingles
        if diff > 1.0 {
            let cut = diff / 2.0;
            emit(
                Level::Info,
                "video.auphonic.trim",
                &format!(
                    "Detected duration difference of {:.2}s. Trimming {:.2}s from start and end...",
                    diff, cut
                ),
                None,
            );

            let trimmed_path = project_paths
                .transcript_dir()
                .join(format!("{}_trimmed.mp3", input_hash));
            let start = cut;
            let end = processed_duration - cut;

            trim_audio(&cache_path, &trimmed_path, start, end)?;

            // Replace cache file with trimmed version
            fs::rename(&trimmed_path, &cache_path)
                .context("Failed to replace cache file with trimmed version")?;
        }
    }

    // Copy to output
    copy_to_output(&cache_path, &input_path)?;

    Ok(())
}

fn copy_to_output(cache_path: &Path, input_path: &Path) -> Result<()> {
    let output_dir = input_path.parent().unwrap_or_else(|| Path::new("."));
    let input_stem = input_path.file_stem().unwrap_or_default();
    let output_filename = format!("{}_processed.mp3", input_stem.to_string_lossy());
    let output_path = output_dir.join(output_filename);

    fs::copy(cache_path, &output_path).with_context(|| {
        format!(
            "Failed to copy result from {} to {}",
            cache_path.display(),
            output_path.display()
        )
    })?;

    emit(
        Level::Success,
        "video.auphonic.success",
        &format!("Saved processed file to {}", output_path.display()),
        None,
    );

    Ok(())
}

async fn create_or_get_preset(client: &Client, api_key: &str) -> Result<String> {
    let preset_name = "Auto Podcast Preset";

    // List presets
    let url = format!("{}/presets.json", BASE_URL);
    let resp = client
        .get(&url)
        .header(AUTHORIZATION, format!("bearer {}", api_key))
        .send()
        .await
        .context("Failed to list presets")?;

    if resp.status().is_success() {
        let json: serde_json::Value = resp.json().await?;
        if let Some(data) = json.get("data").and_then(|d| d.as_array()) {
            for p in data {
                if p.get("preset_name").and_then(|n| n.as_str()) == Some(preset_name) {
                    if let Some(uuid) = p.get("uuid").and_then(|u| u.as_str()) {
                        emit(
                            Level::Info,
                            "video.auphonic.preset",
                            &format!("Found existing preset: {} ({})", preset_name, uuid),
                            None,
                        );
                        return Ok(uuid.to_string());
                    }
                }
            }
        }
    }

    emit(
        Level::Info,
        "video.auphonic.preset",
        "Creating new Auphonic preset...",
        None,
    );

    // Create new preset
    let preset_data = json!({
        "preset_name": preset_name,
        "algorithms": {
            "filtering": true,
            "leveler": true,
            "normloudness": true,
            "loudnesstarget": -19,
            "denoise": true,
            "denoiseamount": 100,
            "silence_cutter": false,
            "filler_cutter": true,
            "cough_cutter": true
        },
        "output_files": [
            {"format": "mp3", "bitrate": "128", "bitrate_mode": "cbr"}
        ]
    });

    let resp = client
        .post(&url)
        .header(AUTHORIZATION, format!("bearer {}", api_key))
        .header(CONTENT_TYPE, "application/json")
        .json(&preset_data)
        .send()
        .await
        .context("Failed to create preset")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Failed to create preset ({}): {}", status, text);
    }

    let json: serde_json::Value = resp.json().await?;
    let uuid = json["data"]["uuid"]
        .as_str()
        .context("New preset has no UUID")?
        .to_string();

    emit(
        Level::Success,
        "video.auphonic.preset",
        &format!("Created new preset: {} ({})", preset_name, uuid),
        None,
    );

    Ok(uuid)
}

async fn start_production(
    client: &Client,
    api_key: &str,
    preset_uuid: &str,
    input_path: &Path,
) -> Result<String> {
    let url = format!("{}/simple/productions.json", BASE_URL);
    let title = input_path.file_stem().unwrap_or_default().to_string_lossy();

    // Read file content
    let file_content = tokio::fs::read(input_path)
        .await
        .context("Failed to read input file")?;
    let file_part = Part::bytes(file_content).file_name(
        input_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
    );

    let form = Form::new()
        .text("preset", preset_uuid.to_string())
        .text("title", title.to_string())
        .text("action", "start")
        .part("input_file", file_part);

    emit(
        Level::Info,
        "video.auphonic.upload",
        &format!("Uploading {} to Auphonic...", input_path.display()),
        None,
    );

    let resp = client
        .post(&url)
        .header(AUTHORIZATION, format!("bearer {}", api_key))
        .multipart(form)
        .send()
        .await
        .context("Failed to start production")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Failed to start production ({}): {}", status, text);
    }

    let json: serde_json::Value = resp.json().await?;
    let uuid = json["data"]["uuid"]
        .as_str()
        .context("Production response has no UUID")?
        .to_string();

    emit(
        Level::Info,
        "video.auphonic.started",
        &format!("Production started with UUID: {}", uuid),
        None,
    );
    emit(
        Level::Info,
        "video.auphonic.link",
        &format!(
            "Track status at: https://auphonic.com/engine/status/{}",
            uuid
        ),
        None,
    );

    Ok(uuid)
}

async fn wait_for_production(client: &Client, api_key: &str, uuid: &str) -> Result<()> {
    let url = format!("{}/production/{}/status.json", BASE_URL, uuid);

    loop {
        let resp = client
            .get(&url)
            .header(AUTHORIZATION, format!("bearer {}", api_key))
            .send()
            .await
            .context("Failed to check status")?;

        if !resp.status().is_success() {
            emit(
                Level::Warn,
                "video.auphonic.status",
                "Failed to check status, retrying...",
                None,
            );
            sleep(Duration::from_secs(10)).await;
            continue;
        }

        let json: serde_json::Value = resp.json().await?;
        let status_data = &json["data"];
        let status_code = status_data["status"].as_i64().unwrap_or(-1);
        let status_string = status_data["status_string"].as_str().unwrap_or("Unknown");

        emit(
            Level::Info,
            "video.auphonic.status",
            &format!("Status: {}", status_string),
            None,
        );

        match status_code {
            3 => return Ok(()), // Done
            2 => anyhow::bail!("Error during processing: {}", status_data["error_message"]),
            _ => sleep(Duration::from_secs(5)).await,
        }
    }
}

async fn download_result(
    client: &Client,
    api_key: &str,
    uuid: &str,
    output_path: &Path,
) -> Result<()> {
    let url = format!("{}/production/{}.json", BASE_URL, uuid);
    let resp = client
        .get(&url)
        .header(AUTHORIZATION, format!("bearer {}", api_key))
        .send()
        .await
        .context("Failed to get production details")?;

    let json: serde_json::Value = resp.json().await?;
    let output_files = json["data"]["output_files"]
        .as_array()
        .context("No output files found")?;

    // Find the first audio file (mp3/m4a) or just take the first one
    let output_file = output_files.first().context("Output files list is empty")?;
    let download_url = output_file["download_url"]
        .as_str()
        .context("No download URL")?;

    let download_url_with_token = if download_url.contains('?') {
        format!("{}&bearer_token={}", download_url, api_key)
    } else {
        format!("{}?bearer_token={}", download_url, api_key)
    };

    emit(
        Level::Info,
        "video.auphonic.download",
        &format!("Download URL: {}", download_url),
        None,
    );
    emit(
        Level::Info,
        "video.auphonic.download",
        "Downloading processed file...",
        None,
    );

    let mut current_url = download_url_with_token;
    let mut attempts = 0;
    let max_attempts = 5;

    loop {
        if attempts >= max_attempts {
            anyhow::bail!("Too many redirects");
        }

        let resp = client
            .get(&current_url)
            .send()
            .await
            .context("Failed to download file")?;

        if resp.status().is_success() {
            let content = resp.bytes().await?;
            fs::write(output_path, content).context("Failed to write output file")?;
            return Ok(());
        } else if resp.status().is_redirection() {
            if let Some(location) = resp.headers().get(reqwest::header::LOCATION) {
                let location_str = location.to_str().context("Invalid Location header")?;
                // Handle relative redirects if necessary, but Auphonic likely returns absolute
                current_url = location_str.to_string();
                attempts += 1;
                emit(
                    Level::Debug,
                    "video.auphonic.redirect",
                    &format!("Redirecting to {}", current_url),
                    None,
                );
                continue;
            }
        }

        anyhow::bail!("Failed to download file: {}", resp.status());
    }
}

fn get_duration(path: &Path) -> Result<f64> {
    let output = std::process::Command::new("ffprobe")
        .args(&[
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(path)
        .output()
        .context("Failed to run ffprobe")?;

    if !output.status.success() {
        anyhow::bail!(
            "ffprobe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let duration_str = String::from_utf8_lossy(&output.stdout);
    let duration: f64 = duration_str
        .trim()
        .parse()
        .context("Failed to parse duration")?;
    Ok(duration)
}

fn trim_audio(input: &Path, output: &Path, start: f64, end: f64) -> Result<()> {
    // ffmpeg -i input -ss start -to end -c copy output
    let output_str = output.to_string_lossy();

    // We need to use -y to overwrite if it exists (though we usually create new temp files)
    let status = std::process::Command::new("ffmpeg")
        .args(&[
            "-y",
            "-i",
            &input.to_string_lossy(),
            "-ss",
            &format!("{}", start),
            "-to",
            &format!("{}", end),
            "-c",
            "copy",
            &output_str,
        ])
        .status()
        .context("Failed to run ffmpeg")?;

    if !status.success() {
        anyhow::bail!("ffmpeg failed to trim audio");
    }
    Ok(())
}
