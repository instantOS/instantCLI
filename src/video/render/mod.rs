mod ffmpeg;
mod paths;
pub mod timeline;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};

use crate::ui::prelude::{Level, emit};

/// Rendering mode for the output video
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderMode {
    /// Standard rendering (same dimensions as source)
    #[default]
    Standard,
    /// Instagram Reels/TikTok (9:16 vertical, 1080x1920)
    Reels,
}

impl RenderMode {
    /// Get target dimensions for this render mode
    pub fn target_dimensions(&self, source_width: u32, source_height: u32) -> (u32, u32) {
        match self {
            RenderMode::Standard => (source_width, source_height),
            RenderMode::Reels => (1080, 1920),
        }
    }

    /// Get output file suffix for this render mode
    pub fn output_suffix(&self) -> &str {
        match self {
            RenderMode::Standard => "_edit",
            RenderMode::Reels => "_reels",
        }
    }

    /// Whether this mode requires letterboxing/pillboxing
    pub fn requires_padding(&self) -> bool {
        matches!(self, RenderMode::Reels)
    }

    /// Get vertical position offset as percentage (0.0 = top, 0.5 = center)
    pub fn vertical_offset_pct(&self) -> f64 {
        match self {
            RenderMode::Standard => 0.5,
            RenderMode::Reels => 0.1, // 10% from top
        }
    }
}

use super::cli::RenderArgs;
use super::config::{VideoConfig, VideoDirectories};
use super::document::{VideoMetadata, VideoSource, parse_video_document};
use super::subtitles::{AssStyle, generate_ass_file, remap_subtitles_to_timeline};

use self::ffmpeg::compiler::FfmpegCompiler;
use self::ffmpeg::services::{
    DefaultMusicSourceResolver, FfmpegRunner, MusicSourceResolver, SystemFfmpegRunner,
};
use super::support::ffmpeg::probe_video_dimensions;

use self::timeline::{Segment, Timeline, Transform};
use super::slides::SlideGenerator;
use super::support::transcript::parse_whisper_json;

trait SlideProvider {
    fn overlay_slide_image(&self, markdown: &str) -> Result<PathBuf>;
    fn standalone_slide_video(&self, markdown: &str, duration: f64) -> Result<PathBuf>;
}

impl SlideProvider for SlideGenerator {
    fn overlay_slide_image(&self, markdown: &str) -> Result<PathBuf> {
        Ok(self.markdown_slide(markdown)?.image_path)
    }

    fn standalone_slide_video(&self, markdown: &str, duration: f64) -> Result<PathBuf> {
        let asset = self.markdown_slide(markdown)?;
        self.ensure_video_for_duration(&asset, duration)
    }
}
use super::planning::{
    StandalonePlan, TimelinePlan, TimelinePlanItem, align_plan_with_subtitles, plan_timeline,
};
use super::support::utils::canonicalize_existing;

macro_rules! log {
    ($level:expr, $code:expr, $($arg:tt)*) => {{
        let message = format!($($arg)*);
        emit($level, $code, &message, None);
    }};
}

fn resolve_source_path(path: &Path, markdown_dir: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(markdown_dir.join(path))
    }
}

fn find_default_source<'a>(
    metadata: &VideoMetadata,
    sources: &'a [VideoSource],
) -> Result<&'a VideoSource> {
    let default_id = metadata
        .default_source
        .as_ref()
        .or_else(|| sources.first().map(|source| &source.id))
        .ok_or_else(|| anyhow!("No video sources available"))?;
    sources
        .iter()
        .find(|source| &source.id == default_id)
        .ok_or_else(|| anyhow!("Default source `{}` not found", default_id))
}

fn validate_timeline_sources(
    document: &super::document::VideoDocument,
    sources: &[VideoSource],
    cues: &[super::support::transcript::TranscriptCue],
) -> Result<()> {
    let mut referenced_sources = std::collections::HashSet::new();
    for block in &document.blocks {
        if let super::document::DocumentBlock::Segment(segment) = block {
            referenced_sources.insert(segment.source_id.clone());
        }
    }

    let mut available_sources = std::collections::HashSet::new();
    for source in sources {
        available_sources.insert(source.id.clone());
    }

    for source_id in &referenced_sources {
        if !available_sources.contains(source_id) {
            bail!("Timeline references unknown source `{}`", source_id);
        }
    }

    let mut cue_sources = std::collections::HashSet::new();
    for cue in cues {
        cue_sources.insert(cue.source_id.clone());
    }

    for source_id in &referenced_sources {
        if !cue_sources.contains(source_id) {
            bail!(
                "No transcript cues loaded for source `{}`; check front matter transcripts",
                source_id
            );
        }
    }

    Ok(())
}

pub fn handle_render(args: RenderArgs) -> Result<()> {
    let runner = SystemFfmpegRunner;
    handle_render_with_services(args, &runner)
}

fn handle_render_with_services(args: RenderArgs, runner: &dyn FfmpegRunner) -> Result<()> {
    let pre_cache_only = args.precache_slides;
    let dry_run = args.dry_run;
    let burn_subtitles = args.subtitles;

    log!(
        Level::Info,
        "video.render.start",
        "Preparing render (reading markdown, transcript, and assets)"
    );

    let markdown_path = canonicalize_existing(&args.markdown)?;
    let markdown_dir = markdown_path.parent().unwrap_or_else(|| Path::new("."));

    let document = load_video_document(&markdown_path)?;
    let sources = resolve_video_sources(&document.metadata, markdown_dir)?;
    if sources.is_empty() {
        bail!("No video sources configured. Add `sources` in front matter before rendering.");
    }
    let cues = load_transcript_cues(&sources, markdown_dir)?;
    validate_timeline_sources(&document, &sources, &cues)?;
    let plan = build_timeline_plan(&document, &cues, &markdown_path)?;

    let default_source = find_default_source(&document.metadata, &sources)?;
    let audio_map = build_audio_source_map(&sources)?;

    // Determine render mode from CLI args
    let render_mode = if args.reels {
        RenderMode::Reels
    } else {
        RenderMode::Standard
    };

    // Warn if subtitles requested but not in reels mode
    if burn_subtitles && render_mode != RenderMode::Reels {
        emit(
            Level::Warn,
            "video.render.subtitles.mode",
            "Subtitles are currently only supported in reels mode (--reels). Ignoring --subtitles flag.",
            None,
        );
    }

    // Automatically enable subtitles for Reels mode
    let burn_subtitles = burn_subtitles || render_mode == RenderMode::Reels;

    let output_path = if pre_cache_only {
        None
    } else {
        Some(paths::resolve_output_path(
            args.out_file.as_ref(),
            &default_source.source,
            markdown_dir,
            render_mode,
        )?)
    };

    if let Some(output_path) = &output_path {
        prepare_output_destination(output_path, &args, &default_source.source)?;
    }

    log!(
        Level::Info,
        "video.render.probe",
        "Probing source video dimensions"
    );
    let (video_width, video_height) = probe_video_dimensions(&default_source.source)?;

    // Use render mode to determine target dimensions
    let (target_width, target_height) = render_mode.target_dimensions(video_width, video_height);

    let generator = SlideGenerator::new(target_width, target_height)?;

    log!(
        Level::Info,
        "video.render.timeline.build",
        "Building render timeline (may generate slides)"
    );
    let (nle_timeline, stats) = build_nle_timeline(plan, &generator, &sources, markdown_dir)?;

    report_timeline_stats(&stats);

    if pre_cache_only {
        emit(
            Level::Success,
            "video.render.precache_only",
            "Prepared slides in cache; skipping final render",
            None,
        );
        return Ok(());
    }

    let Some(output_path) = output_path else {
        bail!("Output path is required when not pre-caching");
    };

    // Generate subtitles if requested and in reels mode
    let subtitle_path = if burn_subtitles && render_mode == RenderMode::Reels {
        log!(
            Level::Info,
            "video.render.subtitles",
            "Generating ASS subtitles for reels mode"
        );
        Some(generate_subtitle_file(
            &nle_timeline,
            &cues,
            &output_path,
            (target_width, target_height),
        )?)
    } else {
        None
    };

    let video_config = VideoConfig::load()?;
    let pipeline = RenderPipeline::new(RenderPipelineParams {
        output: output_path.clone(),
        timeline: nle_timeline,
        render_mode,
        source_width: video_width,
        source_height: video_height,
        config: video_config,
        audio_source: default_source.source.clone(),
        audio_map,
        subtitle_path,
        runner,
    });

    log!(
        Level::Info,
        "video.render.ffmpeg",
        "Preparing ffmpeg pipeline"
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

    log!(
        Level::Info,
        "video.render.execute",
        "Starting ffmpeg render"
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

/// Generate an ASS subtitle file for the timeline.
fn generate_subtitle_file(
    timeline: &Timeline,
    cues: &[super::support::transcript::TranscriptCue],
    output_path: &Path,
    play_res: (u32, u32),
) -> Result<PathBuf> {
    let remapped = remap_subtitles_to_timeline(timeline, cues);

    if remapped.is_empty() {
        emit(
            Level::Warn,
            "video.render.subtitles.empty",
            "No subtitle cues found to burn into video",
            None,
        );
    } else {
        emit(
            Level::Info,
            "video.render.subtitles.count",
            &format!("Remapped {} subtitle entries to timeline", remapped.len()),
            None,
        );
    }

    let style = AssStyle::for_reels(timeline.has_overlays);
    let ass_content = generate_ass_file(&remapped, &style, play_res);

    // Write ASS file next to output with .ass extension
    let ass_path = output_path.with_extension("ass");
    fs::write(&ass_path, &ass_content)
        .with_context(|| format!("Failed to write subtitle file to {}", ass_path.display()))?;

    emit(
        Level::Info,
        "video.render.subtitles.written",
        &format!("Wrote subtitle file to {}", ass_path.display()),
        None,
    );

    Ok(ass_path)
}

pub(super) fn load_video_document(markdown_path: &Path) -> Result<super::document::VideoDocument> {
    log!(
        Level::Info,
        "video.render.markdown.read",
        "Reading markdown from {}",
        markdown_path.display()
    );

    let markdown_contents = fs::read_to_string(markdown_path)
        .with_context(|| format!("Failed to read markdown file {}", markdown_path.display()))?;

    log!(
        Level::Info,
        "video.render.markdown.parse",
        "Parsing markdown into video edit instructions"
    );
    parse_video_document(&markdown_contents, markdown_path)
}

pub(super) fn load_transcript_cues(
    sources: &[VideoSource],
    markdown_dir: &Path,
) -> Result<Vec<super::support::transcript::TranscriptCue>> {
    let mut cues = Vec::new();

    for source in sources {
        let transcript_path = resolve_source_path(&source.transcript, markdown_dir)?;
        let transcript_path = canonicalize_existing(&transcript_path)?;

        log!(
            Level::Info,
            "video.render.transcript.read",
            "Reading transcript for {} from {}",
            source.id,
            transcript_path.display()
        );

        let transcript_contents = fs::read_to_string(&transcript_path).with_context(|| {
            format!(
                "Failed to read transcript file {}",
                transcript_path.display()
            )
        })?;

        log!(
            Level::Info,
            "video.render.transcript.parse",
            "Parsing transcript cues for {}",
            source.id
        );
        let mut parsed = parse_whisper_json(&transcript_contents)?;
        for cue in &mut parsed {
            cue.source_id = source.id.clone();
        }
        cues.extend(parsed);
    }

    Ok(cues)
}

pub(super) fn build_timeline_plan(
    document: &super::document::VideoDocument,
    cues: &[super::support::transcript::TranscriptCue],
    markdown_path: &Path,
) -> Result<TimelinePlan> {
    log!(
        Level::Info,
        "video.render.plan",
        "Planning timeline (selecting clips, overlays, cards)"
    );
    let mut plan = plan_timeline(document)?;

    if plan.items.is_empty() {
        bail!(
            "No renderable blocks found in {}. Ensure the markdown contains timestamp code spans or headings.",
            markdown_path.display()
        );
    }

    log!(
        Level::Info,
        "video.render.plan.align",
        "Aligning planned segments with transcript timing"
    );
    align_plan_with_subtitles(&mut plan, cues)?;

    Ok(plan)
}

pub fn resolve_video_sources(
    metadata: &VideoMetadata,
    markdown_dir: &Path,
) -> Result<Vec<VideoSource>> {
    let sources = paths::resolve_video_sources(metadata, markdown_dir)?;
    let mut resolved = Vec::new();
    for source in sources {
        let resolved_source = resolve_source_path(&source.source, markdown_dir)?;
        let resolved_transcript = resolve_source_path(&source.transcript, markdown_dir)?;
        let resolved_audio = resolve_audio_path(&resolved_source)?;
        let canonical = canonicalize_existing(&resolved_source)?;
        log!(
            Level::Info,
            "video.render.video",
            "Using source {} video {}",
            source.id,
            canonical.display()
        );
        resolved.push(VideoSource {
            source: resolved_source,
            transcript: resolved_transcript,
            audio: resolved_audio,
            ..source
        });
    }

    Ok(resolved)
}

fn resolve_audio_path(video_path: &Path) -> Result<PathBuf> {
    log!(
        Level::Info,
        "video.render.video.hash",
        "Computing hash for cache lookup"
    );

    let video_hash = super::support::utils::compute_file_hash(video_path)?;
    let directories = VideoDirectories::new()?;
    let project_paths = directories.project_paths(&video_hash);
    let transcript_dir = project_paths.transcript_dir();

    // Check for local preprocessed file (WAV) - Preferred
    let local_processed_path = transcript_dir.join(format!("{}_local_processed.wav", video_hash));
    if local_processed_path.exists() {
        emit(
            Level::Info,
            "video.render.audio",
            &format!(
                "Using local preprocessed audio: {}",
                local_processed_path.display()
            ),
            None,
        );
        return Ok(local_processed_path);
    }

    // Check for Auphonic processed file (MP3) - Legacy/Alternative
    let auphonic_processed_path =
        transcript_dir.join(format!("{}_auphonic_processed.mp3", video_hash));

    if auphonic_processed_path.exists() {
        emit(
            Level::Info,
            "video.render.audio",
            &format!(
                "Using Auphonic processed audio: {}",
                auphonic_processed_path.display()
            ),
            None,
        );
        Ok(auphonic_processed_path)
    } else {
        emit(
            Level::Warn,
            "video.render.audio",
            "No preprocessed audio found (local or Auphonic). Using original video audio.",
            None,
        );
        Ok(video_path.to_path_buf())
    }
}

fn build_audio_source_map(
    sources: &[VideoSource],
) -> Result<std::collections::HashMap<String, PathBuf>> {
    let mut audio_map = std::collections::HashMap::new();
    for source in sources {
        let audio_path = resolve_audio_path(&source.source)?;
        audio_map.insert(source.id.clone(), audio_path);
    }
    Ok(audio_map)
}

fn prepare_output_destination(
    output_path: &Path,
    args: &RenderArgs,
    video_path: &Path,
) -> Result<()> {
    if output_path == video_path {
        bail!(
            "Output path {} would overwrite the source video",
            output_path.display()
        );
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
            bail!(
                "Output file {} already exists. Use --force to overwrite.",
                output_path.display()
            );
        }
    }

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output directory {}", parent.display()))?;
    }

    Ok(())
}

fn report_timeline_stats(stats: &TimelineStats) {
    if stats.standalone_count > 0 {
        emit(
            Level::Info,
            "video.render.slides.standalone",
            &format!(
                "Generated {count} standalone slide(s)",
                count = stats.standalone_count
            ),
            None,
        );
    }

    if stats.overlay_count > 0 {
        emit(
            Level::Info,
            "video.render.slides.overlay",
            &format!(
                "Applied {count} overlay slide(s)",
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
}

struct TimelineStats {
    standalone_count: usize,
    overlay_count: usize,
    ignored_count: usize,
}

/// Build an NLE timeline from the timeline plan
fn build_nle_timeline(
    plan: TimelinePlan,
    generator: &dyn SlideProvider,
    sources: &[VideoSource],
    markdown_dir: &Path,
) -> Result<(Timeline, TimelineStats)> {
    let stats = TimelineStats {
        standalone_count: plan.standalone_count,
        overlay_count: plan.overlay_count,
        ignored_count: plan.ignored_count,
    };

    let mut state = TimelineBuildState::new(markdown_dir);

    for item in plan.items {
        state.apply_plan_item(item, generator, sources)?;
    }

    state.finalize();

    // Set has_overlays flag based on plan
    let has_overlays = plan.overlay_count > 0;
    let mut timeline = state.timeline;
    timeline.has_overlays = has_overlays;

    Ok((timeline, stats))
}

struct TimelineBuildState {
    timeline: Timeline,
    current_time: f64,
    music_resolver: Box<dyn MusicSourceResolver>,
    active_music: Option<ActiveMusic>,
}

impl TimelineBuildState {
    fn new(markdown_dir: &Path) -> Self {
        Self {
            timeline: Timeline::new(),
            current_time: 0.0,
            music_resolver: Box::new(DefaultMusicSourceResolver::new(markdown_dir)),
            active_music: None,
        }
    }

    fn apply_plan_item(
        &mut self,
        item: TimelinePlanItem,
        generator: &dyn SlideProvider,
        sources: &[VideoSource],
    ) -> Result<()> {
        match item {
            TimelinePlanItem::Clip(clip_plan) => self.add_clip(clip_plan, generator, sources),
            TimelinePlanItem::Standalone(standalone_plan) => {
                self.add_standalone(standalone_plan, generator)
            }
            TimelinePlanItem::Music(music_plan) => self.add_music_directive(music_plan),
        }
    }

    fn add_clip(
        &mut self,
        clip_plan: super::planning::ClipPlan,
        generator: &dyn SlideProvider,
        sources: &[VideoSource],
    ) -> Result<()> {
        let source = sources
            .iter()
            .find(|source| source.id == clip_plan.source_id)
            .ok_or_else(|| {
                anyhow!(
                    "No source configured for segment source id `{}`",
                    clip_plan.source_id
                )
            })?;
        let source_video = source.source.clone();
        let audio_source = source.audio.clone();
        let duration = clip_plan.end - clip_plan.start;

        let segment = Segment::new_video_subset(
            self.current_time,
            duration,
            clip_plan.start,
            source_video,
            audio_source,
            clip_plan.source_id.clone(),
            None,
            false,
        );
        self.timeline.add_segment(segment);

        if let Some(overlay_plan) = clip_plan.overlay {
            self.add_overlay(&overlay_plan.markdown, duration, generator)?;
        }

        self.current_time += duration;
        Ok(())
    }

    fn add_overlay(
        &mut self,
        markdown: &str,
        duration: f64,
        generator: &dyn SlideProvider,
    ) -> Result<()> {
        let image_path = generator.overlay_slide_image(markdown)?;
        let overlay_segment = Segment::new_image(
            self.current_time,
            duration,
            image_path,
            Some(Transform::with_scale(0.8)),
        );
        self.timeline.add_segment(overlay_segment);
        Ok(())
    }

    fn add_standalone(
        &mut self,
        standalone_plan: StandalonePlan,
        generator: &dyn SlideProvider,
    ) -> Result<()> {
        match standalone_plan {
            StandalonePlan::Heading { level, text, .. } => {
                let heading_level = level.max(1);
                let hashes = "#".repeat(heading_level as usize);
                let markdown_content = format!("{hashes} {}\n", text.trim());
                self.add_standalone_slide(&markdown_content, 2.0, generator)
            }
            StandalonePlan::Pause {
                markdown,
                duration_seconds,
                ..
            } => self.add_standalone_slide(&markdown, duration_seconds, generator),
        }
    }

    fn add_standalone_slide(
        &mut self,
        markdown: &str,
        duration: f64,
        generator: &dyn SlideProvider,
    ) -> Result<()> {
        let video_path = generator.standalone_slide_video(markdown, duration)?;

        let segment = Segment::new_video_subset(
            self.current_time,
            duration,
            0.0,
            video_path.clone(),
            video_path,
            "__slide".to_string(),
            None,
            true,
        );
        self.timeline.add_segment(segment);
        self.current_time += duration;
        Ok(())
    }

    fn add_music_directive(&mut self, music_plan: super::planning::MusicPlan) -> Result<()> {
        finalize_music_segment(
            &mut self.timeline,
            &mut self.active_music,
            self.current_time,
        );
        let resolved = self.music_resolver.resolve(&music_plan.directive)?;
        self.active_music = resolved.map(|path| ActiveMusic {
            path,
            start_time: self.current_time,
        });
        Ok(())
    }

    fn finalize(&mut self) {
        finalize_music_segment(
            &mut self.timeline,
            &mut self.active_music,
            self.current_time,
        );
    }
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
struct RenderPipeline<'a> {
    output: PathBuf,
    timeline: Timeline,
    render_mode: RenderMode,
    source_width: u32,
    source_height: u32,
    config: VideoConfig,
    audio_source: PathBuf,
    audio_map: std::collections::HashMap<String, PathBuf>,
    subtitle_path: Option<PathBuf>,
    runner: &'a dyn FfmpegRunner,
}

struct RenderPipelineParams<'a> {
    output: PathBuf,
    timeline: Timeline,
    render_mode: RenderMode,
    source_width: u32,
    source_height: u32,
    config: VideoConfig,
    audio_source: PathBuf,
    audio_map: std::collections::HashMap<String, PathBuf>,
    subtitle_path: Option<PathBuf>,
    runner: &'a dyn FfmpegRunner,
}

impl<'a> RenderPipeline<'a> {
    fn new(params: RenderPipelineParams<'a>) -> Self {
        Self {
            output: params.output,
            timeline: params.timeline,
            render_mode: params.render_mode,
            source_width: params.source_width,
            source_height: params.source_height,
            config: params.config,
            audio_source: params.audio_source,
            audio_map: params.audio_map,
            subtitle_path: params.subtitle_path,
            runner: params.runner,
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
        self.runner.run(&args)
    }

    fn build_args(&self) -> Result<Vec<String>> {
        let compiler = FfmpegCompiler::new(
            self.render_mode,
            self.source_width,
            self.source_height,
            self.config.clone(),
            self.subtitle_path.clone(),
        );
        Ok(compiler
            .compile(
                self.output.clone(),
                &self.timeline,
                self.audio_source.clone(),
                &self.audio_map,
            )?
            .args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::document::SegmentKind;
    use crate::video::planning::ClipPlan;
    use crate::video::render::timeline::SegmentData;
    use std::path::Path;

    struct StubSlides;

    impl SlideProvider for StubSlides {
        fn overlay_slide_image(&self, _markdown: &str) -> Result<PathBuf> {
            anyhow::bail!("unexpected overlay slide generation")
        }

        fn standalone_slide_video(&self, _markdown: &str, _duration: f64) -> Result<PathBuf> {
            Ok(PathBuf::from("card.mp4"))
        }
    }

    #[test]
    fn inserts_heading_slides_between_clip_segments() {
        let source_video = Path::new("source.mp4");
        let markdown_dir = Path::new(".");

        let plan = TimelinePlan {
            items: vec![
                TimelinePlanItem::Clip(ClipPlan {
                    start: 0.0,
                    end: 12.0,
                    kind: SegmentKind::Dialogue,
                    text: "hello world".to_string(),
                    overlay: None,
                    source_id: "a".to_string(),
                }),
                TimelinePlanItem::Standalone(StandalonePlan::Heading {
                    level: 1,
                    text: "title card".to_string(),
                }),
                TimelinePlanItem::Clip(ClipPlan {
                    start: 12.0,
                    end: 20.0,
                    kind: SegmentKind::Dialogue,
                    text: "this is a test".to_string(),
                    overlay: None,
                    source_id: "a".to_string(),
                }),
            ],
            standalone_count: 1,
            overlay_count: 0,
            ignored_count: 0,
            heading_count: 1,
            segment_count: 2,
        };

        let sources = vec![VideoSource {
            id: "a".to_string(),
            name: Some("source".to_string()),
            source: source_video.to_path_buf(),
            transcript: PathBuf::from("source.json"),
            audio: PathBuf::from("source_audio.wav"),
            hash: None,
        }];

        let (timeline, _stats) =
            build_nle_timeline(plan, &StubSlides, &sources, markdown_dir).unwrap();

        assert_eq!(timeline.segments.len(), 3);

        let SegmentData::VideoSubset {
            start_time: clip1_source_start,
            source_video: clip1_source,
            mute_audio: clip1_mute,
            ..
        } = &timeline.segments[0].data
        else {
            panic!("expected first segment to be a video subset")
        };
        assert!((timeline.segments[0].start_time - 0.0).abs() < 1e-6);
        assert!((timeline.segments[0].duration - 12.0).abs() < 1e-6);
        assert!((*clip1_source_start - 0.0).abs() < 1e-6);
        assert_eq!(clip1_source, &PathBuf::from("source.mp4"));
        assert!(!clip1_mute);

        let SegmentData::VideoSubset {
            start_time: card_source_start,
            source_video: card_source,
            mute_audio: card_mute,
            ..
        } = &timeline.segments[1].data
        else {
            panic!("expected second segment to be a video subset")
        };
        assert!((timeline.segments[1].start_time - 12.0).abs() < 1e-6);
        assert!((timeline.segments[1].duration - 2.0).abs() < 1e-6);
        assert!((*card_source_start - 0.0).abs() < 1e-6);
        assert_eq!(card_source, &PathBuf::from("card.mp4"));
        assert!(*card_mute);

        let SegmentData::VideoSubset {
            start_time: clip2_source_start,
            source_video: clip2_source,
            mute_audio: clip2_mute,
            ..
        } = &timeline.segments[2].data
        else {
            panic!("expected third segment to be a video subset")
        };
        assert!((timeline.segments[2].start_time - 14.0).abs() < 1e-6);
        assert!((timeline.segments[2].duration - 8.0).abs() < 1e-6);
        assert!((*clip2_source_start - 12.0).abs() < 1e-6);
        assert_eq!(clip2_source, &PathBuf::from("source.mp4"));
        assert!(!clip2_mute);
    }

    #[test]
    fn test_render_mode_standard() {
        let mode = RenderMode::Standard;
        assert_eq!(mode.target_dimensions(1920, 1080), (1920, 1080));
        assert_eq!(mode.output_suffix(), "_edit");
        assert!(!mode.requires_padding());
        assert_eq!(mode.vertical_offset_pct(), 0.5);
    }

    #[test]
    fn test_render_mode_reels() {
        let mode = RenderMode::Reels;
        assert_eq!(mode.target_dimensions(1920, 1080), (1080, 1920));
        assert_eq!(mode.output_suffix(), "_reels");
        assert!(mode.requires_padding());
        assert_eq!(mode.vertical_offset_pct(), 0.1);
    }

    #[test]
    fn test_render_mode_default() {
        let mode = RenderMode::default();
        assert_eq!(mode, RenderMode::Standard);
    }
}
