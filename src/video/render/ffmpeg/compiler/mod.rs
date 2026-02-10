mod audio;
mod inputs;
mod overlays;
mod util;
mod video;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;

use super::super::mode::RenderMode;
use crate::video::config::VideoConfig;
use crate::video::render::timeline::Timeline;

use self::util::{categorize_segments, escape_ffmpeg_path};

#[derive(Debug, Clone)]
pub struct FfmpegCompileOutput {
    pub args: Vec<String>,
}

pub struct FfmpegCompiler {
    target_width: u32,
    target_height: u32,
    render_mode: RenderMode,
    config: VideoConfig,
    subtitle_path: Option<PathBuf>,
}

impl FfmpegCompiler {
    pub fn new(
        render_mode: RenderMode,
        source_width: u32,
        source_height: u32,
        config: VideoConfig,
        subtitle_path: Option<PathBuf>,
    ) -> Self {
        let (target_width, target_height) =
            render_mode.target_dimensions(source_width, source_height);
        Self {
            target_width,
            target_height,
            render_mode,
            config,
            subtitle_path,
        }
    }

    pub fn compile(
        &self,
        output: PathBuf,
        timeline: &Timeline,
        audio_source: PathBuf,
        audio_map: &std::collections::HashMap<String, PathBuf>,
    ) -> Result<FfmpegCompileOutput> {
        let mut args = Vec::new();

        let (source_map, source_order) =
            self.build_input_source_map(timeline, &audio_source, audio_map);

        for source in &source_order {
            args.push("-i".to_string());
            args.push(source.to_string_lossy().into_owned());
        }

        let total_duration = timeline.total_duration();

        let filter_complex = self.build_filter_complex(timeline, &source_map, total_duration)?;
        args.push("-filter_complex".to_string());
        args.push(filter_complex);

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
        args.push(output.to_string_lossy().into_owned());

        Ok(FfmpegCompileOutput { args })
    }

    fn build_filter_complex(
        &self,
        timeline: &Timeline,
        source_map: &HashMap<PathBuf, usize>,
        total_duration: f64,
    ) -> Result<String> {
        let mut filters: Vec<String> = Vec::new();

        let (video_segments, overlay_segments, music_segments, broll_segments) =
            categorize_segments(timeline);

        let has_base_track =
            self.build_base_track_filters(&mut filters, &video_segments, source_map)?;

        let mut current_video_label = "concat_v".to_string();

        if !broll_segments.is_empty() {
            current_video_label = self.apply_broll_overlays(
                &mut filters,
                &broll_segments,
                source_map,
                &current_video_label,
            )?;
        }

        if !overlay_segments.is_empty() {
            current_video_label = self.apply_overlays(
                &mut filters,
                &overlay_segments,
                source_map,
                &current_video_label,
            )?;
        }

        if let Some(ass_path) = &self.subtitle_path {
            let escaped_path = escape_ffmpeg_path(ass_path);
            let next_label = "subtitled_v";
            filters.push(format!(
                "[{input}]ass='{path}'[{output}]",
                input = current_video_label,
                path = escaped_path,
                output = next_label
            ));
            current_video_label = next_label.to_string();
        }

        filters.push(format!("[{}]copy[outv]", current_video_label));

        self.build_audio_mix_filters(
            &mut filters,
            &music_segments,
            source_map,
            has_base_track,
            total_duration,
        )?;

        Ok(filters.join("; "))
    }
}
