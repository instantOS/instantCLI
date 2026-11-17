use std::path::Path;

use anyhow::{Context, Result};

use super::audio::AudioTarget;
use crate::assist::geometry::Geometry;

/// Pad video frames without changing the capture bounds.
const EVEN_FRAME_FILTER: &str = "pad=ceil(iw/2)*2:ceil(ih/2)*2";

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum RecordingFormat {
    Mp4,
    WebM,
}

impl RecordingFormat {
    pub(super) fn extension(&self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::WebM => "webm",
        }
    }

    fn codec(&self) -> &'static str {
        match self {
            Self::Mp4 => "libx264",
            Self::WebM => "libvpx-vp9",
        }
    }

    fn codec_params(&self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Mp4 => &[("preset", "veryfast"), ("crf", "23")],
            Self::WebM => &[("crf", "33"), ("b", "0")],
        }
    }
}

pub(super) fn build_ffmpeg_args(
    geometry: Option<&Geometry>,
    format: RecordingFormat,
    output_path: &Path,
    audio_target: Option<&AudioTarget>,
    framerate: Option<i64>,
) -> Result<Vec<String>> {
    let mut args = vec![
        "-f".to_string(),
        "x11grab".to_string(),
        "-draw_mouse".to_string(),
        "1".to_string(),
    ];

    match geometry {
        Some(geom) => {
            args.push("-video_size".to_string());
            args.push(format!("{}x{}", geom.width, geom.height));
            args.push("-i".to_string());
            args.push(format!(":0.0+{},{}", geom.x, geom.y));
        }
        // Fullscreen
        None => {
            args.push("-i".to_string());
            args.push(":0.0".to_string());
        }
    }

    // Audio options
    if let Some(target) = audio_target {
        args.push("-f".to_string());
        args.push("pulse".to_string());
        args.push("-i".to_string());
        args.push(target.name.clone());
        args.push("-ac".to_string());
        args.push("2".to_string());
    }

    // Framerate
    if let Some(fps) = framerate {
        args.push("-framerate".to_string());
        args.push(fps.to_string());
    }

    // Output options
    args.push("-c:v".to_string());
    args.push(format.codec().to_string());

    // Mapping codec params
    for &(key, value) in format.codec_params() {
        if key == "b" {
            args.push("-b:v".to_string());
            args.push(value.to_string());
        } else {
            args.push(format!("-{}", key));
            args.push(value.to_string());
        }
    }

    // Pad the encoded frame, keeping the selected capture region within screen bounds.
    args.extend(["-vf".to_string(), EVEN_FRAME_FILTER.to_string()]);

    // Pixel format
    args.push("-pix_fmt".to_string());
    args.push("yuv420p".to_string());

    // Overwrite output
    args.push("-y".to_string());

    args.push(
        output_path
            .to_str()
            .context("Invalid path encoding")?
            .to_string(),
    );

    Ok(args)
}

pub(super) fn build_wf_recorder_args(
    geometry: Option<&Geometry>,
    format: RecordingFormat,
    output_path: &Path,
    audio_target: Option<&AudioTarget>,
    framerate: Option<i64>,
) -> Result<Vec<String>> {
    let mut args = Vec::new();

    if let Some(geom) = geometry {
        args.push("-g".to_string());
        args.push(geom.to_slurp());
    }

    if let Some(target) = audio_target {
        args.push(format!("--audio={}", target.name));
        if matches!(format, RecordingFormat::WebM) {
            args.push("-C".to_string());
            args.push("libopus".to_string());
        }
    }

    if let Some(fps) = framerate {
        args.push("-r".to_string());
        args.push(fps.to_string());
    }

    args.push("-f".to_string());
    args.push(
        output_path
            .to_str()
            .context("Invalid path encoding")?
            .to_string(),
    );

    args.push("-c".to_string());
    args.push(format.codec().to_string());

    for &(key, value) in format.codec_params() {
        args.push("-p".to_string());
        args.push(format!("{}={}", key, value));
    }

    args.extend(["-F".to_string(), EVEN_FRAME_FILTER.to_string()]);

    args.push("-x".to_string());
    args.push("yuv420p".to_string());

    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn odd_area_is_padded_without_expanding_capture_bounds() {
        let geometry = Geometry {
            x: 1,
            y: 0,
            width: 1919,
            height: 1079,
        };
        let wf = build_wf_recorder_args(
            Some(&geometry),
            RecordingFormat::Mp4,
            Path::new("/tmp/out.mp4"),
            None,
            None,
        )
        .unwrap();
        assert!(wf.windows(2).any(|pair| pair == ["-g", "1,0 1919x1079"]));
        assert!(wf.windows(2).any(|pair| pair == ["-F", EVEN_FRAME_FILTER]));
        let ff = build_ffmpeg_args(
            Some(&geometry),
            RecordingFormat::Mp4,
            Path::new("/tmp/out.mp4"),
            None,
            None,
        )
        .unwrap();
        assert!(
            ff.windows(2)
                .any(|pair| pair == ["-video_size", "1919x1079"])
        );
        assert!(ff.windows(2).any(|pair| pair == ["-vf", EVEN_FRAME_FILTER]));
    }

    #[test]
    fn fullscreen_recording_has_no_geometry_args() {
        let wf = build_wf_recorder_args(
            None,
            RecordingFormat::Mp4,
            Path::new("/tmp/out.mp4"),
            None,
            None,
        )
        .unwrap();
        assert!(!wf.contains(&"-g".to_string()));

        let ff = build_ffmpeg_args(
            None,
            RecordingFormat::Mp4,
            Path::new("/tmp/out.mp4"),
            None,
            None,
        )
        .unwrap();
        assert!(!ff.contains(&"-video_size".to_string()));
        assert!(ff.contains(&":0.0".to_string()));
    }
}
