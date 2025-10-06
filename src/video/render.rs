use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow};

use crate::ui::prelude::{Level, emit};

use super::cli::RenderArgs;
use super::document::{
    DocumentBlock, SegmentBlock, SegmentKind, VideoMetadata, VideoMetadataVideo,
    parse_video_document,
};
use super::utils::canonicalize_existing;

pub fn handle_render(args: RenderArgs) -> Result<()> {
    let markdown_path = canonicalize_existing(&args.markdown)?;
    let markdown_contents = fs::read_to_string(&markdown_path)
        .with_context(|| format!("Failed to read markdown file {}", markdown_path.display()))?;

    let document = parse_video_document(&markdown_contents, &markdown_path)?;

    let markdown_dir = markdown_path.parent().unwrap_or_else(|| Path::new("."));
    let video_path = resolve_video_path(&document.metadata, markdown_dir)?;
    let video_path = canonicalize_existing(&video_path)?;

    let output_path = resolve_output_path(&args, &video_path, markdown_dir)?;
    if output_path == video_path {
        return Err(anyhow!(
            "Output path {} would overwrite the source video",
            output_path.display()
        ));
    }

    if output_path.exists() {
        if args.force {
            fs::remove_file(&output_path).with_context(|| {
                format!(
                    "Failed to remove existing output file {} before overwrite",
                    output_path.display()
                )
            })?;
        } else {
            anyhow::bail!(
                "Output file {} already exists. Use --force to overwrite.",
                output_path.display()
            );
        }
    }

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output directory {}", parent.display()))?;
    }

    let (segments, heading_count, unhandled_count) = collect_timeline_items(&document);
    if segments.is_empty() {
        anyhow::bail!(
            "No timestamp segments found in {}. Ensure the markdown contains `HH:MM:SS.mmm-HH:MM:SS.mmm` code spans.",
            markdown_path.display()
        );
    }

    if heading_count > 0 {
        emit(
            Level::Info,
            "video.render.headings_ignored",
            &format!(
                "Detected {heading_count} heading block(s); title card rendering is not implemented yet"
            ),
            None,
        );
    }

    if unhandled_count > 0 {
        emit(
            Level::Warn,
            "video.render.unhandled_blocks",
            &format!("Ignored {unhandled_count} markdown block(s) that are not yet supported"),
            None,
        );
    }

    let pipeline = RenderPipeline::new(video_path.clone(), output_path.clone(), segments);
    pipeline.execute()?;

    emit(
        Level::Success,
        "video.render.success",
        &format!("Rendered edited timeline to {}", output_path.display()),
        None,
    );

    Ok(())
}

fn resolve_video_path(metadata: &VideoMetadata, markdown_dir: &Path) -> Result<PathBuf> {
    let video_meta: &VideoMetadataVideo = metadata
        .video
        .as_ref()
        .ok_or_else(|| anyhow!("Front matter is missing `video` metadata"))?;

    if let Some(source) = &video_meta.source {
        let resolved = if source.is_absolute() {
            source.clone()
        } else {
            markdown_dir.join(source)
        };
        return Ok(resolved);
    }

    if let Some(name) = &video_meta.name {
        return Ok(markdown_dir.join(name));
    }

    Err(anyhow!(
        "Front matter must include either `video.source` or `video.name` to locate the source video"
    ))
}

fn resolve_output_path(
    args: &RenderArgs,
    video_path: &Path,
    markdown_dir: &Path,
) -> Result<PathBuf> {
    if let Some(provided) = &args.out_file {
        let resolved = if provided.is_absolute() {
            provided.clone()
        } else {
            markdown_dir.join(provided)
        };
        return Ok(resolved);
    }

    let mut output = video_path.to_path_buf();
    let stem = video_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| anyhow!("Video path {} has no valid file name", video_path.display()))?;
    output.set_file_name(format!("{stem}_edit.mp4"));
    Ok(output)
}

fn collect_timeline_items(
    document: &super::document::VideoDocument,
) -> (Vec<ClipSegment>, usize, usize) {
    let mut segments = Vec::new();
    let mut heading_count = 0usize;
    let mut unhandled_count = 0usize;

    for block in &document.blocks {
        match block {
            DocumentBlock::Segment(segment) => segments.push(ClipSegment::from_segment(segment)),
            DocumentBlock::Heading(_) => heading_count += 1,
            DocumentBlock::Unhandled(_) => unhandled_count += 1,
        }
    }

    (segments, heading_count, unhandled_count)
}

#[derive(Debug, Clone)]
struct ClipSegment {
    start: f64,
    end: f64,
    kind: SegmentKind,
    text: String,
    line: usize,
}

impl ClipSegment {
    fn from_segment(segment: &SegmentBlock) -> Self {
        Self {
            start: segment.range.start_seconds(),
            end: segment.range.end_seconds(),
            kind: segment.kind,
            text: segment.text.clone(),
            line: segment.line,
        }
    }
}

struct RenderPipeline {
    input: PathBuf,
    output: PathBuf,
    segments: Vec<ClipSegment>,
}

impl RenderPipeline {
    fn new(input: PathBuf, output: PathBuf, segments: Vec<ClipSegment>) -> Self {
        Self {
            input,
            output,
            segments,
        }
    }

    fn execute(&self) -> Result<()> {
        let mut args = Vec::new();
        args.push("-i".to_string());
        args.push(self.input.to_string_lossy().into_owned());
        args.push("-filter_complex".to_string());
        args.push(self.build_filter_complex());
        args.push("-map".to_string());
        args.push("[outv]".to_string());
        args.push("-map".to_string());
        args.push("[outa]".to_string());
        args.push("-c:v".to_string());
        args.push("libx264".to_string());
        args.push("-preset".to_string());
        args.push("medium".to_string());
        args.push("-crf".to_string());
        args.push("18".to_string());
        args.push("-c:a".to_string());
        args.push("aac".to_string());
        args.push("-b:a".to_string());
        args.push("192k".to_string());
        args.push("-movflags".to_string());
        args.push("+faststart".to_string());
        args.push(self.output.to_string_lossy().into_owned());

        let status = Command::new("ffmpeg")
            .args(&args)
            .status()
            .with_context(|| "Failed to spawn ffmpeg")?;

        if !status.success() {
            anyhow::bail!("ffmpeg exited with status {:?}", status.code());
        }

        Ok(())
    }

    fn build_filter_complex(&self) -> String {
        let mut filters: Vec<String> = Vec::new();
        let mut concat_inputs = String::new();

        for (idx, segment) in self.segments.iter().enumerate() {
            let video_label = format!("v{idx}");
            let audio_label = format!("a{idx}");
            filters.push(format!(
                "[0:v]trim=start={start}:end={end},setpts=PTS-STARTPTS[{video_label}]",
                start = format_time(segment.start),
                end = format_time(segment.end),
            ));
            filters.push(format!(
                "[0:a]atrim=start={start}:end={end},asetpts=PTS-STARTPTS[{audio_label}]",
                start = format_time(segment.start),
                end = format_time(segment.end),
            ));
            concat_inputs.push_str(&format!("[{video_label}][{audio_label}]"));
        }

        filters.push(format!(
            "{inputs}concat=n={segments}:v=1:a=1[outv][outa]",
            inputs = concat_inputs,
            segments = self.segments.len()
        ));

        filters.join("; ")
    }
}

fn format_time(value: f64) -> String {
    format!("{value:.6}")
}
