use std::path::PathBuf;

use anyhow::Result;

use crate::video::config::VideoConfig;
use crate::video::render::ffmpeg::compiler::{FfmpegCompiler, RenderConfig, VideoDimensions};
use crate::video::render::ffmpeg::services::FfmpegRunner;
use crate::video::render::mode::RenderMode;
use crate::video::render::timeline::Timeline;

/// The NLE-based render pipeline
pub(super) struct RenderPipeline<'a> {
    output: PathBuf,
    timeline: Timeline,
    render_mode: RenderMode,
    target_width: u32,
    target_height: u32,
    config: VideoConfig,
    audio_source: PathBuf,
    subtitle_path: Option<PathBuf>,
    runner: &'a dyn FfmpegRunner,
}

pub(super) struct RenderPipelineParams<'a> {
    pub(super) output: PathBuf,
    pub(super) timeline: Timeline,
    pub(super) render_mode: RenderMode,
    pub(super) target_width: u32,
    pub(super) target_height: u32,
    pub(super) config: VideoConfig,
    pub(super) audio_source: PathBuf,
    pub(super) subtitle_path: Option<PathBuf>,
    pub(super) runner: &'a dyn FfmpegRunner,
}

impl<'a> RenderPipeline<'a> {
    pub(super) fn new(params: RenderPipelineParams<'a>) -> Self {
        Self {
            output: params.output,
            timeline: params.timeline,
            render_mode: params.render_mode,
            target_width: params.target_width,
            target_height: params.target_height,
            config: params.config,
            audio_source: params.audio_source,
            subtitle_path: params.subtitle_path,
            runner: params.runner,
        }
    }

    pub(super) fn print_command(&self) -> Result<()> {
        let args = self.build_args()?;
        println!("ffmpeg command that would be executed:");
        println!("ffmpeg {}", args.join(" "));
        Ok(())
    }

    pub(super) fn execute(&self) -> Result<()> {
        let args = self.build_args()?;
        self.runner.run(&args)
    }

    fn build_args(&self) -> Result<Vec<String>> {
        let dimensions = VideoDimensions::new(self.target_width, self.target_height);
        let render_config = RenderConfig::new(
            self.render_mode,
            self.config.clone(),
            self.subtitle_path.clone(),
        );
        let compiler = FfmpegCompiler::new(dimensions, render_config);
        Ok(compiler
            .compile(
                self.output.clone(),
                &self.timeline,
                self.audio_source.clone(),
            )?
            .args)
    }
}
