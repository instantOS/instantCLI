use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};

use crate::ui::prelude::{Level, emit};

use super::cli::RenderArgs;
use super::config::{VideoConfig, VideoDirectories};
use super::document::{VideoMetadata, VideoMetadataVideo, parse_video_document};
use super::music::MusicResolver;
use super::nle_timeline::{Segment, SegmentData, Timeline, Transform};
use super::srt::parse_srt;
use super::timeline::{
    StandalonePlan, TimelinePlan, TimelinePlanItem, align_plan_with_subtitles, plan_timeline,
};
use super::title_card::TitleCardGenerator;
use super::utils::canonicalize_existing;

pub fn handle_render(args: RenderArgs) -> Result<()> {
    let pre_cache_only = args.precache_titlecards;
    let dry_run = args.dry_run;
    let markdown_path = canonicalize_existing(&args.markdown)?;
    let markdown_contents = fs::read_to_string(&markdown_path)
        .with_context(|| format!("Failed to read markdown file {}", markdown_path.display()))?;

    let document = parse_video_document(&markdown_contents, &markdown_path)?;

    let markdown_dir = markdown_path.parent().unwrap_or_else(|| Path::new("."));
    let transcript_path = resolve_transcript_path(&document.metadata, markdown_dir)?;
    let transcript_path = canonicalize_existing(&transcript_path)?;
    let transcript_contents = fs::read_to_string(&transcript_path).with_context(|| {
        format!(
            "Failed to read transcript file {}",
            transcript_path.display()
        )
    })?;
    let cues = parse_srt(&transcript_contents)?;

    let mut plan = plan_timeline(&document)?;

    if plan.items.is_empty() {
        anyhow::bail!(
            "No renderable blocks found in {}. Ensure the markdown contains timestamp code spans or headings.",
            markdown_path.display()
        );
    }

    align_plan_with_subtitles(&mut plan, &cues)?;
    let video_path = resolve_video_path(&document.metadata, markdown_dir)?;
    let video_path = canonicalize_existing(&video_path)?;

    // Resolve Auphonic processed audio
    let video_hash = super::utils::compute_file_hash(&video_path)?;
    let directories = VideoDirectories::new()?;
    let project_paths = directories.project_paths(&video_hash);
    let auphonic_processed_path = project_paths
        .transcript_dir()
        .join(format!("{}_auphonic_processed.mp3", video_hash));

    let audio_path = if auphonic_processed_path.exists() {
        emit(
            Level::Info,
            "video.render.audio",
            &format!(
                "Using Auphonic processed audio: {}",
                auphonic_processed_path.display()
            ),
            None,
        );
        auphonic_processed_path
    } else {
        emit(
            Level::Warn,
            "video.render.audio",
            "Auphonic processed audio not found. Using original video audio.",
            None,
        );
        video_path.clone()
    };

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

    // Build NLE timeline from the plan
    let (nle_timeline, stats) = build_nle_timeline(plan, &generator, &video_path, markdown_dir)?;

    if stats.standalone_count > 0 {
        emit(
            Level::Info,
            "video.render.title_cards",
            &format!(
                "Generated {count} title card(s)",
                count = stats.standalone_count
            ),
            None,
        );
    }

    if stats.overlay_count > 0 {
        emit(
            Level::Info,
            "video.render.title_card_overlays",
            &format!(
                "Applied {count} overlay title card(s)",
                count = stats.overlay_count
            ),
            None,
        );
    }

    if stats.ignored_count > 0 {
        emit(
            Level::Warn,
            "video.render.unhandled_blocks",
            &format!(
                "Ignored {count} markdown block(s) that are not yet supported",
                count = stats.ignored_count
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

    let video_config = VideoConfig::load()?;

    let pipeline = RenderPipeline::new(
        output_path.clone(),
        nle_timeline,
        video_width,
        video_height,
        video_config,
        audio_path,
    );

    if dry_run {
        pipeline.print_command()?;
        emit(
            Level::Info,
            "video.render.dry_run",
            "Dry run completed - ffmpeg command printed above",
            None,
        );
        return Ok(());
    }

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

fn resolve_transcript_path(metadata: &VideoMetadata, markdown_dir: &Path) -> Result<PathBuf> {
    let transcript_meta = metadata
        .transcript
        .as_ref()
        .ok_or_else(|| anyhow!("Front matter is missing `transcript` metadata"))?;

    let source = transcript_meta
        .source
        .as_ref()
        .ok_or_else(|| anyhow!("Front matter is missing `transcript.source`"))?;

    let resolved = if source.is_absolute() {
        source.clone()
    } else {
        markdown_dir.join(source)
    };

    if resolved.exists() {
        return Ok(resolved);
    }

    let Some(video_meta) = metadata.video.as_ref() else {
        return Ok(resolved);
    };
    let Some(hash) = video_meta.hash.as_ref() else {
        return Ok(resolved);
    };

    let directories = VideoDirectories::new()?;
    let project_paths = directories.project_paths(hash);
    let cached_transcript = project_paths.transcript_cache_path();

    if cached_transcript.exists() {
        if let Some(parent) = resolved.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create transcript directory {}", parent.display())
            })?;
        }

        fs::copy(cached_transcript, &resolved).with_context(|| {
            format!(
                "Failed to copy cached transcript from {} to {}",
                cached_transcript.display(),
                resolved.display()
            )
        })?;
    }

    Ok(resolved)
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

struct TimelineStats {
    standalone_count: usize,
    overlay_count: usize,
    ignored_count: usize,
}

/// Build an NLE timeline from the timeline plan
fn build_nle_timeline(
    plan: TimelinePlan,
    generator: &TitleCardGenerator,
    source_video: &Path,
    markdown_dir: &Path,
) -> Result<(Timeline, TimelineStats)> {
    let mut timeline = Timeline::new();
    let mut current_time = 0.0;
    let mut music_resolver = MusicResolver::new(markdown_dir);
    let mut active_music: Option<ActiveMusic> = None;

    for item in plan.items {
        match item {
            TimelinePlanItem::Clip(clip_plan) => {
                let duration = clip_plan.end - clip_plan.start;

                // Add the main video clip segment
                let segment = Segment::new_video_subset(
                    current_time,
                    duration,
                    clip_plan.start,
                    source_video.to_path_buf(),
                    None,
                );
                timeline.add_segment(segment);

                // If there's an overlay, add it as an image segment at the same time
                if let Some(overlay_plan) = clip_plan.overlay {
                    let asset = generator.markdown_card(&overlay_plan.markdown)?;
                    let overlay_segment = Segment::new_image(
                        current_time,
                        duration,
                        asset.image_path.clone(),
                        Some(Transform::with_scale(0.8)), // Default overlay scale
                    );
                    timeline.add_segment(overlay_segment);
                }

                current_time += duration;
            }
            TimelinePlanItem::Standalone(standalone_plan) => match standalone_plan {
                StandalonePlan::Heading { level, text, .. } => {
                    let asset = generator.heading_card(level, &text)?;
                    let video_path = generator.ensure_video_for_duration(&asset, 2.0)?;

                    // Title cards are pre-rendered videos, treat as video segments
                    let segment = Segment::new_video_subset(
                        current_time,
                        2.0,
                        0.0, // Start from beginning of title card video
                        video_path,
                        None,
                    );
                    timeline.add_segment(segment);
                    current_time += 2.0;
                }
                StandalonePlan::Pause { markdown, .. } => {
                    let asset = generator.markdown_card(&markdown)?;
                    let video_path = generator.ensure_video_for_duration(&asset, 5.0)?;

                    // Pause cards are pre-rendered videos
                    let segment = Segment::new_video_subset(
                        current_time,
                        5.0,
                        0.0, // Start from beginning of pause card video
                        video_path,
                        None,
                    );
                    timeline.add_segment(segment);
                    current_time += 5.0;
                }
            },
            TimelinePlanItem::Music(music_plan) => {
                finalize_music_segment(&mut timeline, &mut active_music, current_time);
                let resolved = music_resolver.resolve(&music_plan.directive)?;
                if let Some(path) = resolved {
                    active_music = Some(ActiveMusic {
                        path,
                        start_time: current_time,
                    });
                } else {
                    active_music = None;
                }
            }
        }
    }

    finalize_music_segment(&mut timeline, &mut active_music, current_time);

    let stats = TimelineStats {
        standalone_count: plan.standalone_count,
        overlay_count: plan.overlay_count,
        ignored_count: plan.ignored_count,
    };

    Ok((timeline, stats))
}

struct ActiveMusic {
    path: PathBuf,
    start_time: f64,
}

fn finalize_music_segment(
    timeline: &mut Timeline,
    active: &mut Option<ActiveMusic>,
    end_time: f64,
) {
    if let Some(state) = active.take()
        && end_time > state.start_time
    {
        let duration = end_time - state.start_time;
        timeline.add_segment(Segment::new_music(state.start_time, duration, state.path));
    }
}

/// The NLE-based render pipeline
struct RenderPipeline {
    output: PathBuf,
    timeline: Timeline,
    target_width: u32,
    target_height: u32,
    config: VideoConfig,
    audio_source: PathBuf,
}

impl RenderPipeline {
    fn new(
        output: PathBuf,
        timeline: Timeline,
        target_width: u32,
        target_height: u32,
        config: VideoConfig,
        audio_source: PathBuf,
    ) -> Self {
        Self {
            output,
            timeline,
            target_width,
            target_height,
            config,
            audio_source,
        }
    }

    fn print_command(&self) -> Result<()> {
        let args = self.build_args()?;
        println!("ffmpeg command that would be executed:");
        println!("ffmpeg {}", args.join(" "));
        Ok(())
    }

    fn execute(&self) -> Result<()> {
        let args = self.build_args()?;

        let status = Command::new("ffmpeg")
            .args(&args)
            .status()
            .with_context(|| "Failed to spawn ffmpeg")?;

        if !status.success() {
            anyhow::bail!("ffmpeg exited with status {:?}", status.code());
        }

        Ok(())
    }

    fn build_args(&self) -> Result<Vec<String>> {
        let mut args = Vec::new();

        // Collect all unique source files and assign input indices
        let mut source_map: std::collections::HashMap<PathBuf, usize> =
            std::collections::HashMap::new();
        let mut source_order: Vec<PathBuf> = Vec::new();
        let mut next_index = 0;

        // First pass: identify all unique sources in timeline order
        for segment in &self.timeline.segments {
            if let Some(source) = segment.data.source_path()
                && !source_map.contains_key(source)
            {
                source_map.insert(source.clone(), next_index);
                source_order.push(source.clone());
                next_index += 1;
            }
        }

        // Add the separate audio source if it's different from the video source
        // Actually, we always add it as a separate input to simplify logic,
        // unless we want to be smart about it. Let's just add it.
        let audio_input_index = if !source_map.contains_key(&self.audio_source) {
            source_map.insert(self.audio_source.clone(), next_index);
            source_order.push(self.audio_source.clone());
            next_index += 1;
            next_index - 1
        } else {
            *source_map.get(&self.audio_source).unwrap()
        };

        // Add all input files in the order they were discovered
        for source in &source_order {
            args.push("-i".to_string());
            args.push(source.to_string_lossy().into_owned());
        }

        // Build filter complex
        let filter_complex = self.build_filter_complex(&source_map, audio_input_index)?;
        args.push("-filter_complex".to_string());
        args.push(filter_complex);

        // Map outputs
        args.push("-map".to_string());
        args.push("[outv]".to_string());
        args.push("-map".to_string());
        args.push("[outa]".to_string());

        // Encoding settings
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

        Ok(args)
    }

    fn build_filter_complex(
        &self,
        source_map: &std::collections::HashMap<PathBuf, usize>,
        audio_input_index: usize,
    ) -> Result<String> {
        let mut filters: Vec<String> = Vec::new();

        let (video_segments, overlay_segments, music_segments) = self.categorize_segments();

        let has_base_track = self.build_base_track_filters(
            &mut filters,
            &video_segments,
            source_map,
            audio_input_index,
        )?;

        self.build_overlay_filters_wrapper(&mut filters, &overlay_segments, source_map)?;

        self.build_audio_mix_filters(
            &mut filters,
            &music_segments,
            source_map,
            has_base_track,
        )?;

        Ok(filters.join("; "))
    }

    fn categorize_segments<'a>(
        &'a self,
    ) -> (Vec<&'a Segment>, Vec<&'a Segment>, Vec<&'a Segment>) {
        let mut video = Vec::new();
        let mut overlay = Vec::new();
        let mut music = Vec::new();

        for segment in &self.timeline.segments {
            match &segment.data {
                SegmentData::VideoSubset { .. } => video.push(segment),
                SegmentData::Image { .. } => overlay.push(segment),
                SegmentData::Music { .. } => music.push(segment),
            }
        }
        (video, overlay, music)
    }

    fn build_base_track_filters(
        &self,
        filters: &mut Vec<String>,
        video_segments: &[&Segment],
        source_map: &std::collections::HashMap<PathBuf, usize>,
        audio_input_index: usize,
    ) -> Result<bool> {
        if video_segments.is_empty() {
            return Ok(false);
        }

        // Sort video segments by start time to maintain timeline order
        let mut sorted_video_segments = video_segments.to_vec();
        sorted_video_segments.sort_by(|a, b| a.start_time.partial_cmp(&b.start_time).unwrap());

        // Build video concatenation in timeline order
        let mut concat_inputs = String::new();
        for (idx, segment) in sorted_video_segments.iter().enumerate() {
            if let SegmentData::VideoSubset {
                start_time,
                source_video,
                ..
            } = &segment.data
            {
                let input_index = source_map.get(source_video).unwrap();
                let video_label = format!("v{idx}");
                let audio_label = format!("a{idx}");

                let end_time = start_time + segment.duration;

                // Trim video segment
                filters.push(format!(
                    "[{input}:v]trim=start={start}:end={end},setpts=PTS-STARTPTS[{video}]",
                    input = input_index,
                    start = format_time(*start_time),
                    end = format_time(end_time),
                    video = video_label,
                ));

                // Trim audio segment from the separate audio source
                filters.push(format!(
                    "[{input}:a]atrim=start={start}:end={end},asetpts=PTS-STARTPTS[{audio}]",
                    input = audio_input_index,
                    start = format_time(*start_time),
                    end = format_time(end_time),
                    audio = audio_label,
                ));

                concat_inputs.push_str(&format!(
                    "[{video}][{audio}]",
                    video = video_label,
                    audio = audio_label
                ));
            }
        }

        // Concatenate all video segments
        filters.push(format!(
            "{inputs}concat=n={count}:v=1:a=1[concat_v][concat_a]",
            inputs = concat_inputs,
            count = sorted_video_segments.len()
        ));

        Ok(true)
    }

    fn build_overlay_filters_wrapper(
        &self,
        filters: &mut Vec<String>,
        overlay_segments: &[&Segment],
        source_map: &std::collections::HashMap<PathBuf, usize>,
    ) -> Result<()> {
        if overlay_segments.is_empty() {
            filters.push("[concat_v]copy[outv]".to_string());
        } else {
            // Build overlay application with time-based enabling
            self.apply_overlays(filters, overlay_segments, source_map)?;
        }
        Ok(())
    }

    fn build_audio_mix_filters(
        &self,
        filters: &mut Vec<String>,
        music_segments: &[&Segment],
        source_map: &std::collections::HashMap<PathBuf, usize>,
        has_base_track: bool,
    ) -> Result<()> {
        let mut audio_label: Option<String> = None;

        if has_base_track {
            filters.push("[concat_a]anull[a_base]".to_string());
            audio_label = Some("a_base".to_string());
        }

        if !music_segments.is_empty() {
            let music_label =
                self.build_music_filters(filters, music_segments, source_map)?;
            audio_label = Some(match audio_label {
                Some(base) => {
                    let mixed = "a_mix".to_string();
                    filters.push(format!(
                        "[{base}][{music}]amix=inputs=2:normalize=0:dropout_transition=0[{mixed}]",
                        base = base,
                        music = music_label,
                        mixed = mixed,
                    ));
                    mixed
                }
                None => music_label,
            });
        }

        let final_audio = if let Some(label) = audio_label {
            label
        } else {
            let total_duration = self.timeline.total_duration();
            let duration = format_time(total_duration);
            filters.push(format!(
                "anullsrc=r=48000:cl=stereo,atrim=duration={duration}[a_silence]",
            ));
            "a_silence".to_string()
        };

        filters.push(format!("[{label}]anull[outa]", label = final_audio));
        Ok(())
    }

    fn apply_overlays(
        &self,
        filters: &mut Vec<String>,
        overlay_segments: &[&Segment],
        source_map: &std::collections::HashMap<PathBuf, usize>,
    ) -> Result<()> {
        let mut current_video_label = "concat_v".to_string();

        for (idx, segment) in overlay_segments.iter().enumerate() {
            if let SegmentData::Image {
                source_image,
                transform,
            } = &segment.data
            {
                let input_index = source_map.get(source_image).unwrap();
                let overlay_label = format!("overlay_{idx}");
                let output_label = format!("overlaid_{idx}");

                // Process the overlay image with transform
                let scale_factor = transform.as_ref().and_then(|t| t.scale).unwrap_or(0.8);

                filters.push(format!(
                    "[{input}:v]scale=w=ceil({width}*{scale}/2)*2:h=-1:flags=lanczos,setsar=1,format=rgba[{overlay}]",
                    input = input_index,
                    width = self.target_width,
                    scale = scale_factor,
                    overlay = overlay_label,
                ));

                // Apply overlay with time-based enabling
                let enable_condition =
                    format!("between(t,{},{})", segment.start_time, segment.end_time());

                filters.push(format!(
                    "[{video}][{overlay}]overlay=x=(W-w)/2:y=(H-h)/2:enable='{condition}'[{output}]",
                    video = current_video_label,
                    overlay = overlay_label,
                    condition = enable_condition,
                    output = output_label,
                ));

                current_video_label = output_label;
            }
        }

        // Final output
        filters.push(format!("[{}]copy[outv]", current_video_label));
        Ok(())
    }

    fn build_music_filters(
        &self,
        filters: &mut Vec<String>,
        music_segments: &[&Segment],
        source_map: &std::collections::HashMap<PathBuf, usize>,
    ) -> Result<String> {
        let mut labels = Vec::new();
        let music_volume = self.config.music_volume();

        for (idx, segment) in music_segments.iter().enumerate() {
            if segment.duration <= 0.0 {
                continue;
            }

            if let SegmentData::Music { audio_source } = &segment.data {
                let input_index = source_map.get(audio_source).ok_or_else(|| {
                    anyhow!(
                        "No ffmpeg input available for background music {}",
                        audio_source.display()
                    )
                })?;

                let label = format!("music_{idx}");
                let duration_str = format_time(segment.duration);
                let delay_ms = ((segment.start_time * 1000.0).round()).max(0.0) as u64;

                filters.push(format!(
                    "[{input}:a]atrim=start=0:end={duration},asetpts=PTS-STARTPTS,apad=pad_dur={duration},atrim=duration={duration},aresample=async=1:first_pts=0,adelay={delay}|{delay},volume={volume}[{label}]",
                    input = input_index,
                    duration = duration_str,
                    delay = delay_ms,
                    volume = format!("{:.6}", music_volume),
                    label = label,
                ));

                labels.push(label);
            }
        }

        match labels.as_slice() {
            [] => bail!("No music segments available to build audio filters"),
            [label] => Ok(label.to_string()),
            _ => {
                let mut inputs = String::new();
                for label in &labels {
                    inputs.push_str(&format!("[{label}]"));
                }
                let output_label = "music_mix".to_string();
                filters.push(format!(
                    "{inputs}amix=inputs={count}:normalize=0:dropout_transition=0[{output}]",
                    inputs = inputs,
                    count = labels.len(),
                    output = output_label,
                ));
                Ok(output_label)
            }
        }
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
