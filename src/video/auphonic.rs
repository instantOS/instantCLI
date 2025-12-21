use anyhow::{Context, Result};
use reqwest::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use reqwest::multipart::{Form, Part};
use serde_json::json;
use std::fs;
use std::path::Path;
use std::time::Duration;
use tokio::time::sleep;

use super::cli::AuphonicArgs;
use super::config::{VideoConfig, VideoDirectories};
use super::utils::{canonicalize_existing, compute_file_hash};
use crate::ui::prelude::{Level, emit};

const BASE_URL: &str = "https://auphonic.com/api";

pub(crate) async fn verify_api_key(client: &Client, api_key: &str) -> Result<()> {
    let url = format!("{}/presets.json", BASE_URL);
    let resp = client
        .get(&url)
        .header(AUTHORIZATION, format!("bearer {}", api_key))
        .send()
        .await
        .context("Failed to connect to Auphonic API")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Auphonic API error ({}): {}", status, text);
    }

    let _json: serde_json::Value = resp
        .json()
        .await
        .context("Failed to parse Auphonic response")?;

    Ok(())
}

pub async fn handle_auphonic(args: AuphonicArgs) -> Result<()> {
    let input_path = canonicalize_existing(&args.input_file)?;

    // Load config to check for API key if we need to process
    // We do this check inside process_with_auphonic, but we might need to pass args

    let processed_path =
        process_with_auphonic(&input_path, args.force, args.api_key, args.preset).await?;

    // Copy to output
    copy_to_output(&processed_path, &input_path)?;

    Ok(())
}

pub async fn process_with_auphonic(
    input_path: &Path,
    force: bool,
    api_key_arg: Option<String>,
    preset_arg: Option<String>,
) -> Result<std::path::PathBuf> {
    let input_hash = compute_file_hash(input_path)?;

    let directories = VideoDirectories::new()?;
    let project_paths = directories.project_paths(&input_hash);
    project_paths.ensure_directories()?;

    let raw_cache_file_name = format!("{}_auphonic_raw.mp3", input_hash);
    let raw_cache_path = project_paths.transcript_dir().join(&raw_cache_file_name);

    let processed_cache_file_name = format!("{}_auphonic_processed.mp3", input_hash);
    let processed_cache_path = project_paths
        .transcript_dir()
        .join(&processed_cache_file_name);

    // Determine if input is audio or video (extract audio if video)
    let is_audio = input_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            ["mp3", "wav", "flac", "m4a", "ogg", "aac", "wma", "aiff"]
                .contains(&e.to_lowercase().as_str())
        })
        .unwrap_or(false);

    let extracted_audio_path = project_paths
        .transcript_dir()
        .join(format!("{}_extracted.mp3", input_hash));

    // Ensure extracted audio exists if input is video
    let upload_input_path = if is_audio {
        input_path.to_path_buf()
    } else {
        if !extracted_audio_path.exists() || force {
            emit(
                Level::Info,
                "video.auphonic.extract",
                &format!("Extracting audio from {}...", input_path.display()),
                None,
            );
            extract_audio(input_path, &extracted_audio_path)?;
        }
        extracted_audio_path.clone()
    };

    // Step 1: Ensure raw Auphonic result exists
    if !raw_cache_path.exists() || force {
        // Load config
        let config = VideoConfig::load()?;
        let api_key = api_key_arg
            .or(config.auphonic_api_key)
            .context("Auphonic API key not found. Please provide it via --api-key or in ~/.config/instant/video.toml")?;

        let client = Client::new();

        // Get or create preset
        let preset_uuid = if let Some(uuid) = preset_arg.or(config.auphonic_preset_uuid) {
            uuid
        } else {
            create_or_get_preset(&client, &api_key).await?
        };

        let title = input_path.file_stem().unwrap_or_default().to_string_lossy();

        // Start production
        let production_uuid =
            start_production(&client, &api_key, &preset_uuid, &upload_input_path, &title).await?;

        // Poll status
        wait_for_production(&client, &api_key, &production_uuid).await?;

        // Download result
        download_result(&client, &api_key, &production_uuid, &raw_cache_path).await?;
    } else {
        emit(
            Level::Info,
            "video.auphonic.cached",
            &format!(
                "Using cached raw Auphonic result at {}",
                raw_cache_path.display()
            ),
            None,
        );
    }

    // Step 2: Process/Trim the raw result to remove Auphonic free tier jingles
    // We re-process if the processed file doesn't exist OR if force is enabled
    if !processed_cache_path.exists() || force {
        // Compare against the extracted/uploaded audio, not the original video
        // This ensures accurate duration comparison
        let original_duration =
            get_duration(&upload_input_path).context("Failed to get original audio duration")?;
        let raw_duration = get_duration(&raw_cache_path).context("Failed to get raw Auphonic duration")?;

        emit(
            Level::Debug,
            "video.auphonic.duration",
            &format!(
                "Original audio: {:.2}s, Auphonic output: {:.2}s, Diff: {:.2}s",
                original_duration, raw_duration, raw_duration - original_duration
            ),
            None,
        );

        if raw_duration > original_duration {
            let diff = raw_duration - original_duration;
            // Auphonic free tier jingles are ~1-2s each at start and end
            // Use 0.5s threshold to catch smaller jingles reliably
            if diff > 0.5 {
                let cut = diff / 2.0;
                emit(
                    Level::Info,
                    "video.auphonic.trim",
                    &format!(
                        "Detected jingles (duration diff: {:.2}s). Trimming {:.2}s from start and end...",
                        diff, cut
                    ),
                    None,
                );

                let start = cut;
                let end = raw_duration - cut;

                trim_audio(&raw_cache_path, &processed_cache_path, start, end)?;
            } else {
                // Just copy if no significant difference
                emit(
                    Level::Debug,
                    "video.auphonic.no_trim",
                    &format!("Duration diff {:.2}s below threshold, no trimming needed", diff),
                    None,
                );
                fs::copy(&raw_cache_path, &processed_cache_path)?;
            }
        } else {
            // Raw is shorter or equal, just copy
            emit(
                Level::Debug,
                "video.auphonic.no_trim",
                "Auphonic output not longer than original, no trimming needed",
                None,
            );
            fs::copy(&raw_cache_path, &processed_cache_path)?;
        }
    } else {
        emit(
            Level::Info,
            "video.auphonic.cached",
            &format!(
                "Using cached processed result at {}",
                processed_cache_path.display()
            ),
            None,
        );
    }

    Ok(processed_cache_path)
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

    let expected_algorithms = json!({
        "filtering": true,
        "leveler": true,
        "normloudness": true,
        "loudnesstarget": -19,
        "denoise": true,
        "denoiseamount": 100,
        "silence_cutter": false,
        "filler_cutter": false,
        "cough_cutter": false
    });

    let expected_output_files = json!([
        {"format": "mp3", "bitrate": "128", "bitrate_mode": "cbr"}
    ]);

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
                if p.get("preset_name").and_then(|n| n.as_str()) == Some(preset_name)
                    && let Some(uuid) = p.get("uuid").and_then(|u| u.as_str())
                {
                    emit(
                        Level::Info,
                        "video.auphonic.preset",
                        &format!("Found existing preset: {} ({})", preset_name, uuid),
                        None,
                    );

                    // Verify configuration
                    let preset_url = format!("{}/preset/{}.json", BASE_URL, uuid);
                    let preset_resp = client
                        .get(&preset_url)
                        .header(AUTHORIZATION, format!("bearer {}", api_key))
                        .send()
                        .await
                        .context("Failed to get preset details")?;

                    if preset_resp.status().is_success() {
                        let preset_json: serde_json::Value = preset_resp.json().await?;
                        let current_algorithms = &preset_json["data"]["algorithms"];
                        let current_output_files = &preset_json["data"]["output_files"];

                        // Simple comparison - might need more robust check if API returns extra fields
                        // But for now, let's check if our expected fields match
                        let mut needs_update = false;

                        if let Some(current_obj) = current_algorithms.as_object() {
                            if let Some(expected_obj) = expected_algorithms.as_object() {
                                for (k, v) in expected_obj {
                                    if current_obj.get(k) != Some(v) {
                                        emit(
                                            Level::Debug,
                                            "video.auphonic.preset",
                                            &format!(
                                                "Config mismatch for {}: expected {}, got {:?}",
                                                k,
                                                v,
                                                current_obj.get(k)
                                            ),
                                            None,
                                        );
                                        needs_update = true;
                                        break;
                                    }
                                }
                            } else {
                                needs_update = true; // Expected algorithms is an object, current is not
                            }
                        } else if expected_algorithms.is_object() {
                            needs_update = true; // Current algorithms is not an object, but expected is
                        }

                        // Compare output files (simple string comparison of the serialized JSON)
                        if *current_output_files != expected_output_files {
                            emit(
                                Level::Debug,
                                "video.auphonic.preset",
                                &format!(
                                    "Output files mismatch: expected {}, got {}",
                                    expected_output_files, current_output_files
                                ),
                                None,
                            );
                            needs_update = true;
                        }

                        if needs_update {
                            emit(
                                Level::Info,
                                "video.auphonic.preset",
                                "Preset configuration mismatch. Updating preset...",
                                None,
                            );

                            let update_data = json!({
                                "algorithms": expected_algorithms,
                                "output_files": expected_output_files
                            });

                            let update_resp = client
                                .post(&preset_url)
                                .header(AUTHORIZATION, format!("bearer {}", api_key))
                                .header(CONTENT_TYPE, "application/json")
                                .json(&update_data)
                                .send()
                                .await
                                .context("Failed to update preset")?;

                            if !update_resp.status().is_success() {
                                let text = update_resp.text().await.unwrap_or_default();
                                emit(
                                    Level::Warn,
                                    "video.auphonic.preset",
                                    &format!("Failed to update preset: {}", text),
                                    None,
                                );
                            } else {
                                emit(
                                    Level::Success,
                                    "video.auphonic.preset",
                                    "Preset updated successfully.",
                                    None,
                                );
                            }
                        } else {
                            emit(
                                Level::Debug,
                                "video.auphonic.preset",
                                "Preset configuration matches.",
                                None,
                            );
                        }
                    }

                    return Ok(uuid.to_string());
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
        "algorithms": expected_algorithms,
        "output_files": expected_output_files
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
    title: &str,
) -> Result<String> {
    let url = format!("{}/simple/productions.json", BASE_URL);

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
        } else if resp.status().is_redirection()
            && let Some(location) = resp.headers().get(reqwest::header::LOCATION)
        {
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

        anyhow::bail!("Failed to download file: {}", resp.status());
    }
}

fn get_duration(path: &Path) -> Result<f64> {
    let output = std::process::Command::new("ffprobe")
        .args([
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

fn extract_audio(input: &Path, output: &Path) -> Result<()> {
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-i",
            &input.to_string_lossy(),
            "-vn",
            "-map",
            "0:a:0",
            "-c:a",
            "libmp3lame",
            "-q:a",
            "2",
            &output.to_string_lossy(),
        ])
        .status()
        .context("Failed to run ffmpeg for audio extraction")?;

    if !status.success() {
        anyhow::bail!("ffmpeg failed to extract audio");
    }
    Ok(())
}

fn trim_audio(input: &Path, output: &Path, start: f64, end: f64) -> Result<()> {
    // ffmpeg -i input -ss start -to end -c:a libmp3lame -q:a 2 output
    // Removing -c copy to ensure precision, using high quality VBR MP3
    let output_str = output.to_string_lossy();

    let status = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-i",
            &input.to_string_lossy(),
            "-ss",
            &format!("{}", start),
            "-to",
            &format!("{}", end),
            "-c:a",
            "libmp3lame",
            "-q:a",
            "2",
            &output_str,
        ])
        .status()
        .context("Failed to run ffmpeg")?;

    if !status.success() {
        anyhow::bail!("ffmpeg failed to trim audio");
    }
    Ok(())
}
