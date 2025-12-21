use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

pub fn probe_duration_seconds(path: &Path) -> Result<f64> {
    let output = Command::new("ffprobe")
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
        .with_context(|| format!("Failed to run ffprobe for {}", path.display()))?;

    if !output.status.success() {
        anyhow::bail!(
            "ffprobe failed for {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let duration_str = String::from_utf8_lossy(&output.stdout);
    let duration: f64 = duration_str
        .trim()
        .parse()
        .context("Failed to parse ffprobe duration as f64")?;

    Ok(duration)
}

pub fn probe_video_dimensions(video_path: &Path) -> Result<(u32, u32)> {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("v:0")
        .arg("-show_entries")
        .arg("stream=width,height")
        .arg("-of")
        .arg("csv=s=x:p=0")
        .arg(video_path)
        .output()
        .with_context(|| {
            format!(
                "Failed to probe video dimensions for {}",
                video_path.display()
            )
        })?;

    if !output.status.success() {
        anyhow::bail!(
            "ffprobe exited with status {:?} while probing {}",
            output.status.code(),
            video_path.display()
        );
    }

    let stdout = String::from_utf8(output.stdout)
        .context("ffprobe returned non-UTF8 output for video dimensions")?;
    let value = stdout.trim();
    let mut parts = value.split('x');

    let width_str = parts.next().ok_or_else(|| {
        anyhow::anyhow!("ffprobe did not return width for {}", video_path.display())
    })?;
    let height_str = parts.next().ok_or_else(|| {
        anyhow::anyhow!("ffprobe did not return height for {}", video_path.display())
    })?;

    let width: u32 = width_str.parse().with_context(|| {
        format!(
            "Unable to parse ffprobe width '{}' for {}",
            width_str,
            video_path.display()
        )
    })?;
    let height: u32 = height_str.parse().with_context(|| {
        format!(
            "Unable to parse ffprobe height '{}' for {}",
            height_str,
            video_path.display()
        )
    })?;

    Ok((width, height))
}

pub fn extract_audio_to_mp3(input: &Path, output: &Path) -> Result<()> {
    let status = Command::new("ffmpeg")
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
        .with_context(|| {
            format!(
                "Failed to run ffmpeg to extract audio from {}",
                input.display()
            )
        })?;

    if !status.success() {
        anyhow::bail!("ffmpeg failed to extract audio from {}", input.display());
    }

    Ok(())
}

pub fn trim_audio_mp3(
    input: &Path,
    output: &Path,
    start_seconds: f64,
    end_seconds: f64,
) -> Result<()> {
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-i",
            &input.to_string_lossy(),
            "-ss",
            &format!("{}", start_seconds),
            "-to",
            &format!("{}", end_seconds),
            "-c:a",
            "libmp3lame",
            "-q:a",
            "2",
            &output.to_string_lossy(),
        ])
        .status()
        .with_context(|| {
            format!(
                "Failed to run ffmpeg to trim audio from {}",
                input.display()
            )
        })?;

    if !status.success() {
        anyhow::bail!("ffmpeg failed to trim audio from {}", input.display());
    }

    Ok(())
}
