use std::path::PathBuf;

use anyhow::Result;

use crate::video::render::ffmpeg::compiler::{FfmpegCompiler, RenderConfig, VideoDimensions};
use crate::video::render::ffmpeg::services::FfmpegRunner;
use crate::video::render::timeline::Timeline;

pub(super) struct RenderPipeline<'a> {
    pub(super) output: PathBuf,
    pub(super) timeline: Timeline,
    pub(super) dimensions: VideoDimensions,
    pub(super) render_config: RenderConfig,
    pub(super) audio_source: PathBuf,
    pub(super) runner: &'a dyn FfmpegRunner,
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
        self.runner.run(&args)
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
}
