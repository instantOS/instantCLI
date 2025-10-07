use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow};

use crate::ui::prelude::{Level, emit};

use super::cli::RenderArgs;
use super::document::{SegmentKind, VideoMetadata, VideoMetadataVideo, parse_video_document};
use super::timeline::{StandalonePlan, TimelinePlan, TimelinePlanItem, plan_timeline};
use super::title_card::{TitleCardAsset, TitleCardGenerator};
use super::utils::canonicalize_existing;

pub fn handle_render(args: RenderArgs) -> Result<()> {
    let pre_cache_only = args.precache_titlecards;
    let markdown_path = canonicalize_existing(&args.markdown)?;
    let markdown_contents = fs::read_to_string(&markdown_path)
        .with_context(|| format!("Failed to read markdown file {}", markdown_path.display()))?;

    let document = parse_video_document(&markdown_contents, &markdown_path)?;
    let plan = plan_timeline(&document)?;

    if plan.items.is_empty() {
        anyhow::bail!(
            "No renderable blocks found in {}. Ensure the markdown contains timestamp code spans or headings.",
            markdown_path.display()
        );
    }

    let markdown_dir = markdown_path.parent().unwrap_or_else(|| Path::new("."));
    let video_path = resolve_video_path(&document.metadata, markdown_dir)?;
    let video_path = canonicalize_existing(&video_path)?;

    let output_path = if pre_cache_only {
        None
    } else {
        Some(resolve_output_path(&args, &video_path, markdown_dir)?)
    };

    if let Some(output_path) = &output_path {
        if output_path == &video_path {
            return Err(anyhow!(
                "Output path {} would overwrite the source video",
                output_path.display()
            ));
        }

        if output_path.exists() {
            if args.force {
                fs::remove_file(output_path).with_context(|| {
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
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create output directory {}", parent.display())
            })?;
        }
    }

    let (video_width, video_height) = probe_video_dimensions(&video_path)?;
    let generator = TitleCardGenerator::new(video_width, video_height)?;
    let timeline = materialize_timeline(plan, &generator)?;

    if timeline.standalone_count > 0 {
        emit(
            Level::Info,
            "video.render.title_cards",
            &format!(
                "Generated {count} title card(s)",
                count = timeline.standalone_count
            ),
            None,
        );
    }

    if timeline.overlay_count > 0 {
        emit(
            Level::Info,
            "video.render.title_card_overlays",
            &format!(
                "Applied {count} overlay title card(s)",
                count = timeline.overlay_count
            ),
            None,
        );
    }

    if timeline.ignored_count > 0 {
        emit(
            Level::Warn,
            "video.render.unhandled_blocks",
            &format!(
                "Ignored {count} markdown block(s) that are not yet supported",
                count = timeline.ignored_count
            ),
            None,
        );
    }

    if pre_cache_only {
        emit(
            Level::Success,
            "video.render.precache_only",
            "Prepared title cards and overlays in cache; skipping final render",
            None,
        );
        return Ok(());
    }

    let output_path = output_path.expect("output path is required when not pre-caching");

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

pub(super) fn resolve_video_path(metadata: &VideoMetadata, markdown_dir: &Path) -> Result<PathBuf> {
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
    standalone_count: usize,
    overlay_count: usize,
    ignored_count: usize,
}

fn materialize_timeline(
    plan: TimelinePlan,
    generator: &TitleCardGenerator,
) -> Result<TimelineCollection> {
    let mut items = Vec::new();

    for item in plan.items {
        match item {
            TimelinePlanItem::Clip(clip_plan) => {
                let overlay = match clip_plan.overlay {
                    Some(overlay_plan) => {
                        let asset = generator.markdown_card(&overlay_plan.markdown)?;
                        Some(OverlaySegment {
                            asset,
                            line: overlay_plan.line,
                        })
                    }
                    None => None,
                };

                items.push(TimelineItem::Clip(ClipSegment {
                    start: clip_plan.start,
                    end: clip_plan.end,
                    kind: clip_plan.kind,
                    text: clip_plan.text,
                    line: clip_plan.line,
                    overlay,
                }));
            }
            TimelinePlanItem::Standalone(standalone_plan) => match standalone_plan {
                StandalonePlan::Heading { level, text, line } => {
                    let asset = generator.heading_card(level, &text)?;
                    let video_path = generator.ensure_video_for_duration(&asset, 2.0)?;
                    items.push(TimelineItem::TitleCard(TitleCardSegment {
                        path: video_path,
                        duration: 2.0,
                        text,
                        line,
                    }));
                }
                StandalonePlan::Pause {
                    markdown,
                    display_text,
                    line,
                } => {
                    let asset = generator.markdown_card(&markdown)?;
                    let video_path = generator.ensure_video_for_duration(&asset, 5.0)?;
                    items.push(TimelineItem::TitleCard(TitleCardSegment {
                        path: video_path,
                        duration: 5.0,
                        text: display_text,
                        line,
                    }));
                }
            },
        }
    }

    Ok(TimelineCollection {
        items,
        standalone_count: plan.standalone_count,
        overlay_count: plan.overlay_count,
        ignored_count: plan.ignored_count,
    })
}

#[derive(Debug, Clone)]
struct ClipSegment {
    start: f64,
    end: f64,
    kind: SegmentKind,
    text: String,
    line: usize,
    overlay: Option<OverlaySegment>,
}

#[derive(Debug, Clone)]
struct TitleCardSegment {
    path: PathBuf,
    duration: f64,
    text: String,
    line: usize,
}

#[derive(Debug, Clone)]
struct OverlaySegment {
    asset: TitleCardAsset,
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

enum TimelineBinding {
    Clip { overlay_input: Option<usize> },
    TitleCard { input_index: usize },
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

        let mut bindings = Vec::with_capacity(self.timeline.len());
        let mut next_index = 1usize;

        for item in &self.timeline {
            match item {
                TimelineItem::Clip(segment) => {
                    if let Some(overlay) = &segment.overlay {
                        args.push("-loop".to_string());
                        args.push("1".to_string());
                        args.push("-i".to_string());
                        args.push(overlay.asset.image_path.to_string_lossy().into_owned());
                        bindings.push(TimelineBinding::Clip {
                            overlay_input: Some(next_index),
                        });
                        next_index += 1;
                    } else {
                        bindings.push(TimelineBinding::Clip {
                            overlay_input: None,
                        });
                    }
                }
                TimelineItem::TitleCard(card) => {
                    args.push("-i".to_string());
                    args.push(card.path.to_string_lossy().into_owned());
                    bindings.push(TimelineBinding::TitleCard {
                        input_index: next_index,
                    });
                    next_index += 1;
                }
            }
        }

        args.push("-filter_complex".to_string());
        args.push(self.build_filter_complex(&bindings));
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

    fn build_filter_complex(&self, bindings: &[TimelineBinding]) -> String {
        let mut filters: Vec<String> = Vec::new();
        let mut concat_inputs = String::new();
        let overlay_scale_expr = format!("ceil({width}*0.6/2)*2", width = self.target_width);

        for (idx, (item, binding)) in self.timeline.iter().zip(bindings.iter()).enumerate() {
            let audio_label = format!("a{idx}");

            match (item, binding) {
                (TimelineItem::Clip(segment), TimelineBinding::Clip { overlay_input }) => {
                    let base_label = format!("v{idx}_base");
                    filters.push(format!(
                        "[0:v]trim=start={start}:end={end},setpts=PTS-STARTPTS[{base}]",
                        start = format_time(segment.start),
                        end = format_time(segment.end),
                        base = base_label,
                    ));

                    let final_label = if let Some(overlay_index) = overlay_input {
                        let overlay_label = format!("ov{idx}");
                        filters.push(format!(
                            "[{input}:v]scale=w={scale}:h=-1:flags=lanczos,setsar=1,format=rgba,colorchannelmixer=aa=0.85,setpts=PTS-STARTPTS[{overlay}]",
                            input = overlay_index,
                            scale = overlay_scale_expr.as_str(),
                            overlay = overlay_label,
                        ));
                        let final_label = format!("v{idx}");
                        filters.push(format!(
                            "[{base}][{overlay}]overlay=x=(W-w)/2:y=(H-h)/2:shortest=1[{final}]",
                            base = base_label,
                            overlay = overlay_label,
                            final = final_label,
                        ));
                        final_label
                    } else {
                        base_label
                    };

                    filters.push(format!(
                        "[0:a]atrim=start={start}:end={end},asetpts=PTS-STARTPTS[{audio}]",
                        start = format_time(segment.start),
                        end = format_time(segment.end),
                        audio = audio_label,
                    ));

                    concat_inputs.push_str(&format!(
                        "[{video}][{audio}]",
                        video = final_label,
                        audio = audio_label
                    ));
                }
                (TimelineItem::TitleCard(_), TimelineBinding::TitleCard { input_index }) => {
                    let video_label = format!("v{idx}");
                    filters.push(format!(
                        "[{input}:v]scale={width}:{height},setsar=1,setpts=PTS-STARTPTS[{video}]",
                        input = input_index,
                        width = self.target_width,
                        height = self.target_height,
                        video = video_label,
                    ));
                    filters.push(format!(
                        "[{input}:a]asetpts=PTS-STARTPTS[{audio}]",
                        input = input_index,
                        audio = audio_label,
                    ));
                    concat_inputs.push_str(&format!(
                        "[{video}][{audio}]",
                        video = video_label,
                        audio = audio_label
                    ));
                }
                _ => unreachable!("Timeline and bindings variant mismatch"),
            }
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
