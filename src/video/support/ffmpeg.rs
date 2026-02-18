use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::{Command, Output};

#[derive(Debug, Clone, Copy)]
pub struct EncodingProfile {
    pub video_codec: &'static str,
    pub preset: &'static str,
    pub crf: &'static str,
    pub audio_codec: &'static str,
    pub audio_bitrate: &'static str,
    pub pix_fmt: Option<&'static str>,
    pub movflags: Option<&'static str>,
}

pub const PROFILE_H264_AAC_QUALITY_FASTSTART: EncodingProfile = EncodingProfile {
    video_codec: "libx264",
    preset: "medium",
    crf: "18",
    audio_codec: "aac",
    audio_bitrate: "192k",
    pix_fmt: None,
    movflags: Some("+faststart"),
};

pub const PROFILE_SLIDE_VIDEO: EncodingProfile = EncodingProfile {
    video_codec: "libx264",
    preset: "medium",
    crf: "18",
    audio_codec: "aac",
    audio_bitrate: "192k",
    pix_fmt: Some("yuv420p"),
    movflags: None,
};

impl EncodingProfile {
    pub fn push_to(&self, args: &mut Vec<String>) {
        args.push("-c:v".to_string());
        args.push(self.video_codec.to_string());
        args.push("-preset".to_string());
        args.push(self.preset.to_string());
        args.push("-crf".to_string());
        args.push(self.crf.to_string());
        if let Some(pix_fmt) = self.pix_fmt {
            args.push("-pix_fmt".to_string());
            args.push(pix_fmt.to_string());
        }
        args.push("-c:a".to_string());
        args.push(self.audio_codec.to_string());
        args.push("-b:a".to_string());
        args.push(self.audio_bitrate.to_string());
        if let Some(movflags) = self.movflags {
            args.push("-movflags".to_string());
            args.push(movflags.to_string());
        }
    }
}

fn run_ffprobe(args: &[&str], path: &Path, ctx: &str) -> Result<Output> {
    let output = Command::new("ffprobe")
        .args(args)
        .arg(path)
        .output()
        .with_context(|| format!("Failed to run ffprobe for {}", path.display()))?;

    if !output.status.success() {
        bail!(
            "ffprobe failed {} for {}: {}",
            ctx,
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(output)
}

fn ffprobe_stdout_utf8(output: Output) -> Result<String> {
    String::from_utf8(output.stdout).context("ffprobe returned non-UTF8 output")
}

pub fn run_ffmpeg_output(args: &[&str], ctx: &str) -> Result<()> {
    let output = Command::new("ffmpeg")
        .args(args)
        .output()
        .with_context(|| format!("Failed to spawn ffmpeg for {}", ctx))?;

    if !output.status.success() {
        bail!(
            "ffmpeg failed {}: {}",
            ctx,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(())
}

pub fn probe_duration_seconds(path: &Path) -> Result<f64> {
    let output = run_ffprobe(
        &[
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ],
        path,
        "to get duration",
    )?;

    let duration_str = ffprobe_stdout_utf8(output)?;
    let duration: f64 = duration_str
        .trim()
        .parse()
        .context("Failed to parse ffprobe duration as f64")?;

    Ok(duration)
}

pub fn probe_video_dimensions(video_path: &Path) -> Result<(u32, u32)> {
    let output = run_ffprobe(
        &[
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "csv=s=x:p=0",
        ],
        video_path,
        "to get dimensions",
    )?;

    let stdout = ffprobe_stdout_utf8(output)?;
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
    run_ffmpeg_output(
        &[
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
        ],
        &format!("to extract audio from {}", input.display()),
    )
}

pub fn trim_audio_mp3(
    input: &Path,
    output: &Path,
    start_seconds: f64,
    end_seconds: f64,
) -> Result<()> {
    run_ffmpeg_output(
        &[
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
        ],
        &format!("to trim audio from {}", input.display()),
    )
}

/// Comprehensive video/audio metadata for display purposes
#[derive(Debug, Clone, Default)]
pub struct MediaMetadata {
    pub duration_seconds: Option<f64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub framerate: Option<f64>,
    pub bitrate_kbps: Option<u64>,
    pub audio_sample_rate: Option<u32>,
    pub audio_channels: Option<u32>,
}

impl MediaMetadata {
    /// Format duration as HH:MM:SS or MM:SS
    pub fn duration_display(&self) -> Option<String> {
        self.duration_seconds.map(|secs| {
            let total = secs as u64;
            let hours = total / 3600;
            let minutes = (total % 3600) / 60;
            let seconds = total % 60;
            if hours > 0 {
                format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
            } else {
                format!("{:02}:{:02}", minutes, seconds)
            }
        })
    }

    /// Format resolution as WxH
    pub fn resolution_display(&self) -> Option<String> {
        match (self.width, self.height) {
            (Some(w), Some(h)) => Some(format!("{}x{}", w, h)),
            _ => None,
        }
    }

    /// Format bitrate nicely
    pub fn bitrate_display(&self) -> Option<String> {
        self.bitrate_kbps.map(|kbps| {
            if kbps >= 1000 {
                format!("{:.1} Mbps", kbps as f64 / 1000.0)
            } else {
                format!("{} kbps", kbps)
            }
        })
    }

    /// Format framerate
    pub fn framerate_display(&self) -> Option<String> {
        self.framerate.map(|fps| format!("{:.2} fps", fps))
    }

    /// Check if this is audio-only (no video stream)
    pub fn is_audio_only(&self) -> bool {
        self.width.is_none() && self.height.is_none() && self.video_codec.is_none()
    }
}

/// Probe comprehensive metadata from a video or audio file.
/// This is designed to be fast and fail gracefully for preview purposes.
pub fn probe_media_metadata(path: &Path) -> MediaMetadata {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration,bit_rate",
            "-show_entries",
            "stream=codec_name,codec_type,width,height,r_frame_rate,sample_rate,channels",
            "-of",
            "json",
        ])
        .arg(path)
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return MediaMetadata::default(),
    };

    let json_str = match String::from_utf8(output.stdout) {
        Ok(s) => s,
        Err(_) => return MediaMetadata::default(),
    };

    parse_ffprobe_json(&json_str)
}

fn parse_ffprobe_json(json_str: &str) -> MediaMetadata {
    let mut metadata = MediaMetadata::default();

    // Parse JSON manually to avoid adding dependencies
    if let Some(duration) = extract_json_string(json_str, "duration") {
        metadata.duration_seconds = duration.parse().ok();
    }

    if let Some(bit_rate) = extract_json_string(json_str, "bit_rate")
        && let Ok(bps) = bit_rate.parse::<u64>()
    {
        metadata.bitrate_kbps = Some(bps / 1000);
    }

    // Parse streams - find video and audio streams
    let streams_start = json_str.find("\"streams\"");
    if let Some(start) = streams_start {
        let streams_section = &json_str[start..];

        // Find video stream info
        if let Some(video_idx) = streams_section.find("\"codec_type\": \"video\"") {
            let stream_start = streams_section[..video_idx].rfind('{').unwrap_or(0);
            let stream_section = &streams_section[stream_start..];

            if let Some(codec) = extract_json_string(stream_section, "codec_name") {
                metadata.video_codec = Some(codec);
            }
            if let Some(width) = extract_json_number(stream_section, "width") {
                metadata.width = Some(width as u32);
            }
            if let Some(height) = extract_json_number(stream_section, "height") {
                metadata.height = Some(height as u32);
            }
            if let Some(fps_str) = extract_json_string(stream_section, "r_frame_rate") {
                metadata.framerate = parse_framerate(&fps_str);
            }
        }

        // Find audio stream info
        if let Some(audio_idx) = streams_section.find("\"codec_type\": \"audio\"") {
            let stream_start = streams_section[..audio_idx].rfind('{').unwrap_or(0);
            let stream_section = &streams_section[stream_start..];

            if let Some(codec) = extract_json_string(stream_section, "codec_name") {
                metadata.audio_codec = Some(codec);
            }
            if let Some(sample_rate) = extract_json_string(stream_section, "sample_rate") {
                metadata.audio_sample_rate = sample_rate.parse().ok();
            }
            if let Some(channels) = extract_json_number(stream_section, "channels") {
                metadata.audio_channels = Some(channels as u32);
            }
        }
    }

    metadata
}

fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{}\": \"", key);
    let start = json.find(&pattern)? + pattern.len();
    let end = json[start..].find('"')? + start;
    Some(json[start..end].to_string())
}

fn extract_json_number(json: &str, key: &str) -> Option<i64> {
    let pattern = format!("\"{}\": ", key);
    let start = json.find(&pattern)? + pattern.len();
    let rest = &json[start..];
    let end = rest.find(|c: char| !c.is_ascii_digit() && c != '-')?;
    rest[..end].parse().ok()
}

fn parse_framerate(fps_str: &str) -> Option<f64> {
    // Format is typically "30/1" or "30000/1001"
    let parts: Vec<&str> = fps_str.split('/').collect();
    if parts.len() == 2 {
        let num: f64 = parts[0].parse().ok()?;
        let den: f64 = parts[1].parse().ok()?;
        if den > 0.0 {
            return Some(num / den);
        }
    }
    fps_str.parse().ok()
}
