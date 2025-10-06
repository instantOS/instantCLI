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
use super::title_card::TitleCardGenerator;
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

    let (video_width, video_height) = probe_video_dimensions(&video_path)?;
    let generator = TitleCardGenerator::new(video_width, video_height)?;
    let timeline = collect_timeline_items(&document, &generator)?;

    if timeline.items.is_empty() {
        anyhow::bail!(
            "No renderable blocks found in {}. Ensure the markdown contains timestamp code spans or headings.",
            markdown_path.display()
        );
    }

    if timeline.heading_count > 0 {
        emit(
            Level::Info,
            "video.render.title_cards",
            &format!(
                "Generated {count} title card(s)",
                count = timeline.heading_count
            ),
            None,
        );
    }

    if timeline.unhandled_count > 0 {
        emit(
            Level::Warn,
            "video.render.unhandled_blocks",
            &format!(
                "Ignored {count} markdown block(s) that are not yet supported",
                count = timeline.unhandled_count
            ),
            None,
        );
    }

    let pipeline = RenderPipeline::new(
        video_path.clone(),
        output_path.clone(),
        timeline.items,
        video_width,
        video_height,
    );
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

struct TimelineCollection {
    items: Vec<TimelineItem>,
    heading_count: usize,
    unhandled_count: usize,
}

fn collect_timeline_items(
    document: &super::document::VideoDocument,
    generator: &TitleCardGenerator,
) -> Result<TimelineCollection> {
    let mut items = Vec::new();
    let mut heading_count = 0usize;
    let mut unhandled_count = 0usize;

    for block in &document.blocks {
        match block {
            DocumentBlock::Segment(segment) => {
                items.push(TimelineItem::Clip(ClipSegment::from_segment(segment)));
            }
            DocumentBlock::Heading(heading) => {
                let asset = generator.generate(heading.level, &heading.text)?;
                heading_count += 1;
                items.push(TimelineItem::TitleCard(TitleCardSegment {
                    path: asset.video_path,
                    duration: asset.duration,
                    level: heading.level,
                    text: heading.text.clone(),
                    line: heading.line,
                }));
            }
            DocumentBlock::Unhandled(_) => unhandled_count += 1,
        }
    }

    Ok(TimelineCollection {
        items,
        heading_count,
        unhandled_count,
    })
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

#[derive(Debug, Clone)]
struct TitleCardSegment {
    path: PathBuf,
    duration: f64,
    level: u32,
    text: String,
    line: usize,
}

enum TimelineItem {
    Clip(ClipSegment),
    TitleCard(TitleCardSegment),
}

struct RenderPipeline {
    input: PathBuf,
    output: PathBuf,
    timeline: Vec<TimelineItem>,
    target_width: u32,
    target_height: u32,
}

impl RenderPipeline {
    fn new(
        input: PathBuf,
        output: PathBuf,
        timeline: Vec<TimelineItem>,
        target_width: u32,
        target_height: u32,
    ) -> Self {
        Self {
            input,
            output,
            timeline,
            target_width,
            target_height,
        }
    }

    fn execute(&self) -> Result<()> {
        let mut args = Vec::new();
        args.push("-i".to_string());
        args.push(self.input.to_string_lossy().into_owned());

        let mut input_indices = Vec::with_capacity(self.timeline.len());
        let mut additional_inputs = Vec::new();
        let mut next_index = 1usize;

        for item in &self.timeline {
            match item {
                TimelineItem::Clip(_) => input_indices.push(0),
                TimelineItem::TitleCard(card) => {
                    input_indices.push(next_index);
                    additional_inputs.push(card.path.clone());
                    next_index += 1;
                }
            }
        }

        for path in &additional_inputs {
            args.push("-i".to_string());
            args.push(path.to_string_lossy().into_owned());
        }

        args.push("-filter_complex".to_string());
        args.push(self.build_filter_complex(&input_indices));
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

    fn build_filter_complex(&self, input_indices: &[usize]) -> String {
        let mut filters: Vec<String> = Vec::new();
        let mut concat_inputs = String::new();

        for (idx, item) in self.timeline.iter().enumerate() {
            let input_index = input_indices[idx];
            let video_label = format!("v{idx}");
            let audio_label = format!("a{idx}");

            match item {
                TimelineItem::Clip(segment) => {
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
                }
                TimelineItem::TitleCard(_) => {
                    filters.push(format!(
                        "[{input}:v]scale={width}:{height},setsar=1,setpts=PTS-STARTPTS[{video_label}]",
                        input = input_index,
                        width = self.target_width,
                        height = self.target_height,
                    ));
                    filters.push(format!(
                        "[{input}:a]asetpts=PTS-STARTPTS[{audio_label}]",
                        input = input_index,
                    ));
                }
            }

            concat_inputs.push_str(&format!("[{video_label}][{audio_label}]"));
        }

        filters.push(format!(
            "{inputs}concat=n={segments}:v=1:a=1[outv][outa]",
            inputs = concat_inputs,
            segments = self.timeline.len()
        ));

        filters.join("; ")
    }
}

fn format_time(value: f64) -> String {
    format!("{value:.6}")
}

fn probe_video_dimensions(video_path: &Path) -> Result<(u32, u32)> {
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
        .with_context(|| "Failed to probe video dimensions with ffprobe")?;

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
    let width_str = parts
        .next()
        .ok_or_else(|| anyhow!("ffprobe did not return width for {}", video_path.display()))?;
    let height_str = parts
        .next()
        .ok_or_else(|| anyhow!("ffprobe did not return height for {}", video_path.display()))?;

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
