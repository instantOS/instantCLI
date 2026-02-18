use std::path::PathBuf;

use anyhow::Result;

use crate::video::render::ffmpeg::compiler::{FfmpegCompiler, RenderConfig, VideoDimensions};
use crate::video::render::ffmpeg::services::{FfmpegRunOptions, FfmpegRunner, PreviewPlayer};
use crate::video::render::timeline::Timeline;

pub(super) struct RenderPipeline<'a> {
    pub(super) output: PathBuf,
    pub(super) timeline: Timeline,
    pub(super) dimensions: VideoDimensions,
    pub(super) render_config: RenderConfig,
    pub(super) audio_source: PathBuf,
    pub(super) runner: &'a dyn FfmpegRunner,
    pub(super) verbose: bool,
}

impl<'a> RenderPipeline<'a> {
    pub(super) fn print_command(&self) -> Result<()> {
        let args = self.build_args()?;
        println!("ffmpeg command that would be executed:");
        println!("ffmpeg {}", args.join(" "));
        Ok(())
    }

    pub(super) fn execute(&self) -> Result<()> {
        let args = self.build_args()?;
        let options = FfmpegRunOptions::new(Some(self.timeline.total_duration()), self.verbose);
        self.runner.run(&args, options)
    }

    pub(super) fn execute_preview(&self) -> Result<()> {
        let args = self.build_preview_args()?;
        let options = FfmpegRunOptions::new(Some(self.timeline.total_duration()), self.verbose);
        self.runner.run(&args, options)
    }

    pub(super) fn preview(&self, player: &dyn PreviewPlayer) -> Result<()> {
        let args = self.build_preview_args()?;
        player.play(&args)
    }

    pub(super) fn preview_with_seek(
        &self,
        player: &dyn PreviewPlayer,
        seek_time: Option<f64>,
    ) -> Result<()> {
        let args = self.build_preview_args()?;
        player.play_with_seek(&args, seek_time)
    }

    fn build_args(&self) -> Result<Vec<String>> {
        let compiler = FfmpegCompiler::new(self.dimensions, self.render_config.clone());
        Ok(compiler
            .compile(
                self.output.clone(),
                &self.timeline,
                self.audio_source.clone(),
            )?
            .args)
    }

    fn build_preview_args(&self) -> Result<Vec<String>> {
        let compiler = FfmpegCompiler::new(self.dimensions, self.render_config.clone());
        compiler
            .compile_preview(&self.timeline, self.audio_source.clone())
            .map(|o| o.args)
    }
}
