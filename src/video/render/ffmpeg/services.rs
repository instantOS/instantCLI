use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::video::document::MusicDirective;
use crate::video::support::music::MusicResolver;

pub trait FfmpegRunner {
    fn run(&self, args: &[String]) -> Result<()>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemFfmpegRunner;

impl FfmpegRunner for SystemFfmpegRunner {
    fn run(&self, args: &[String]) -> Result<()> {
        let output = Command::new("ffmpeg")
            .args(args)
            .output()
            .with_context(|| "Failed to spawn ffmpeg")?;

        if !output.status.success() {
            bail!(
                "ffmpeg exited with status {:?}: {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr).trim()
            );
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
    pub fn new(project_dir: &Path) -> Self {
        Self {
            resolver: MusicResolver::new(project_dir),
        }
    }
}

impl MusicSourceResolver for DefaultMusicSourceResolver {
    fn resolve(&mut self, directive: &MusicDirective) -> Result<Option<std::path::PathBuf>> {
        self.resolver.resolve(directive)
    }
}
