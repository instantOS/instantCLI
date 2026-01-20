use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

use crate::video::document::MusicDirective;
use crate::video::support::music::MusicResolver;

pub trait FfmpegRunner {
    fn run(&self, args: &[String]) -> Result<()>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemFfmpegRunner;

impl FfmpegRunner for SystemFfmpegRunner {
    fn run(&self, args: &[String]) -> Result<()> {
        let status = Command::new("ffmpeg")
            .args(args)
            .status()
            .with_context(|| "Failed to spawn ffmpeg")?;

        if !status.success() {
            anyhow::bail!("ffmpeg exited with status {:?}", status.code());
        }

        Ok(())
    }
}

pub trait MusicSourceResolver {
    fn resolve(&mut self, directive: &MusicDirective) -> Result<Option<std::path::PathBuf>>;
}

pub struct DefaultMusicSourceResolver {
    resolver: MusicResolver,
}

impl DefaultMusicSourceResolver {
    pub fn new(markdown_dir: &Path) -> Self {
        Self {
            resolver: MusicResolver::new(markdown_dir),
        }
    }
}

impl MusicSourceResolver for DefaultMusicSourceResolver {
    fn resolve(&mut self, directive: &MusicDirective) -> Result<Option<std::path::PathBuf>> {
        self.resolver.resolve(directive)
    }
}
