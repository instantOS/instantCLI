mod audio;
mod graph;
mod inputs;
mod overlays;
mod util;
mod video;

#[cfg(test)]
mod tests;

use std::path::PathBuf;

use anyhow::Result;

use self::graph::FilterGraph;
use self::inputs::SourceMap;

use super::super::mode::RenderMode;
use crate::video::config::VideoConfig;
use crate::video::render::timeline::Timeline;
use crate::video::support::ffmpeg::PROFILE_H264_AAC_QUALITY_FASTSTART;

use self::util::escape_ffmpeg_path;

#[derive(Debug, Clone)]
pub struct FfmpegCompileOutput {
    pub args: Vec<String>,
}

/// Video dimensions (width x height in pixels).
#[derive(Debug, Clone, Copy)]
pub struct VideoDimensions {
    pub width: u32,
    pub height: u32,
}

impl VideoDimensions {
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

/// Configuration for video rendering.
#[derive(Debug, Clone)]
pub struct RenderConfig {
    pub render_mode: RenderMode,
    pub config: VideoConfig,
    pub subtitle_path: Option<PathBuf>,
}

impl RenderConfig {
    pub fn new(
        render_mode: RenderMode,
        config: VideoConfig,
        subtitle_path: Option<PathBuf>,
    ) -> Self {
        Self {
            render_mode,
            config,
            subtitle_path,
        }
    }
}

pub struct FfmpegCompiler {
    target_width: u32,
    target_height: u32,
    render_mode: RenderMode,
    config: VideoConfig,
    subtitle_path: Option<PathBuf>,
}

impl FfmpegCompiler {
    pub fn new(target_dimensions: VideoDimensions, render_config: RenderConfig) -> Self {
        Self {
            target_width: target_dimensions.width,
            target_height: target_dimensions.height,
            render_mode: render_config.render_mode,
            config: render_config.config,
            subtitle_path: render_config.subtitle_path,
        }
    }

    pub fn compile(
        &self,
        output: PathBuf,
        timeline: &Timeline,
        audio_source: PathBuf,
    ) -> Result<FfmpegCompileOutput> {
        let mut args = Vec::new();

        let source_map = SourceMap::build(timeline, &audio_source, false);
        args.extend(source_map.input_args());

        let total_duration = timeline.total_duration();

        let filter_complex = self.build_filter_complex(timeline, &source_map, total_duration)?;
        args.push("-filter_complex".to_string());
        args.push(filter_complex);

        args.push("-map".to_string());
        args.push("[outv]".to_string());
        args.push("-map".to_string());
        args.push("[outa]".to_string());

        // A filter with multiple inputs (notably sidechaincompress/amix) may
        // otherwise keep the mux alive after the edited video timeline ends.
        args.push("-shortest".to_string());

        PROFILE_H264_AAC_QUALITY_FASTSTART.push_to(&mut args);
        args.push(output.to_string_lossy().into_owned());

        Ok(FfmpegCompileOutput { args })
    }

    pub fn compile_preview(
        &self,
        timeline: &Timeline,
        audio_source: PathBuf,
    ) -> Result<FfmpegCompileOutput> {
        let mut args = Vec::new();

        let source_map = SourceMap::build(timeline, &audio_source, true);
        args.extend(source_map.input_args());

        let total_duration = timeline.total_duration();

        let filter_complex = self.build_filter_complex(timeline, &source_map, total_duration)?;
        args.push("-filter_complex".to_string());
        args.push(filter_complex);

        args.push("-map".to_string());
        args.push("[outv]".to_string());
        args.push("-map".to_string());
        args.push("[outa]".to_string());
        args.push("-shortest".to_string());

        // Fast encoding settings for real-time preview
        args.push("-c:v".to_string());
        args.push("libx264".to_string());
        args.push("-preset".to_string());
        args.push("ultrafast".to_string());
        args.push("-crf".to_string());
        args.push("28".to_string());
        args.push("-c:a".to_string());
        args.push("aac".to_string());
        args.push("-b:a".to_string());
        args.push("128k".to_string());
        args.push("-pix_fmt".to_string());
        args.push("yuv420p".to_string());

        // Output format and destination are set by the runner (MpvPreviewRunner)

        Ok(FfmpegCompileOutput { args })
    }

    fn build_filter_complex(
        &self,
        timeline: &Timeline,
        source_map: &SourceMap,
        total_duration: f64,
    ) -> Result<String> {
        let mut graph = FilterGraph::new();

        let video_segments = timeline.video_segments();
        let overlay_segments = timeline.overlay_segments();
        let music_segments = timeline.music_segments();
        let broll_segments = timeline.broll_segments();

        let Some((mut current_video, base_audio)) =
            self.build_base_track_filters(&mut graph, &video_segments, source_map)?
        else {
            anyhow::bail!("Cannot render a timeline without a playable base video track");
        };

        if !broll_segments.is_empty() {
            current_video =
                self.apply_broll_overlays(&mut graph, &broll_segments, source_map, current_video)?;
        }

        if !overlay_segments.is_empty() {
            current_video =
                self.apply_overlays(&mut graph, &overlay_segments, source_map, current_video)?;
        }

        if let Some(ass_path) = &self.subtitle_path {
            let escaped_path = escape_ffmpeg_path(ass_path);
            current_video = graph.video_filter(
                current_video,
                format!("ass='{escaped_path}'"),
                "subtitled_v",
            );
        }

        graph.output_video(current_video);

        self.build_audio_mix_filters(
            &mut graph,
            &music_segments,
            source_map,
            Some(base_audio),
            total_duration,
        )?;

        Ok(graph.finish())
    }
}
