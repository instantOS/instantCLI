use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use indicatif::{ProgressBar, ProgressStyle};

use crate::video::document::MusicDirective;
use crate::video::support::music::MusicResolver;

pub trait FfmpegRunner {
    fn run(&self, args: &[String], options: FfmpegRunOptions) -> Result<()>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemFfmpegRunner;

#[derive(Debug, Clone, Default)]
pub struct FfmpegRunOptions {
    pub total_duration: Option<f64>,
    pub verbose: bool,
}

impl FfmpegRunOptions {
    pub fn new(total_duration: Option<f64>, verbose: bool) -> Self {
        Self {
            total_duration,
            verbose,
        }
    }
}

impl FfmpegRunner for SystemFfmpegRunner {
    fn run(&self, args: &[String], options: FfmpegRunOptions) -> Result<()> {
        let mut child = Command::new("ffmpeg")
            .args(args)
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| "Failed to spawn ffmpeg")?;

        let stderr = child.stderr.take().expect("stderr was piped");

        let pb = if let Some(duration) = options.total_duration {
            let pb = ProgressBar::new((duration * 1000.0) as u64);
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>8}/{len:8} ({eta}) {msg}")
                    .unwrap()
                    .progress_chars("#>-"),
            );
            pb.set_message("rendering".to_string());
            Some(pb)
        } else {
            None
        };

        let mut last_line = String::new();
        let mut error_lines: Vec<String> = Vec::new();
        let result = read_ffmpeg_stderr(
            stderr,
            options.verbose,
            &pb,
            &mut last_line,
            &mut error_lines,
        );

        let status = child.wait().context("Failed to wait for ffmpeg")?;
        result?;

        if let Some(pb) = pb {
            pb.finish_with_message("done");
        }

        if !status.success() {
            let error_msg = if !error_lines.is_empty() {
                error_lines.join("\n")
            } else {
                last_line
            };
            bail!(
                "ffmpeg exited with status {:?}: {}",
                status.code(),
                error_msg.trim()
            );
        }

        Ok(())
    }
}

fn read_ffmpeg_stderr<R: Read>(
    mut stderr: R,
    verbose: bool,
    pb: &Option<ProgressBar>,
    last_line: &mut String,
    error_lines: &mut Vec<String>,
) -> Result<()> {
    let mut buffer = [0u8; 4096];
    let mut accumulated = String::new();

    loop {
        let bytes_read = stderr
            .read(&mut buffer)
            .context("Failed to read ffmpeg stderr")?;
        if bytes_read == 0 {
            break;
        }

        let chunk = String::from_utf8_lossy(&buffer[..bytes_read]);
        accumulated.push_str(&chunk);

        while let Some(pos) = accumulated.find(|c| c == '\r' || c == '\n') {
            let line = accumulated[..pos].to_string();
            accumulated = accumulated[pos + 1..].to_string();

            if line.is_empty() {
                continue;
            }

            *last_line = line.clone();

            if verbose {
                eprintln!("{}", line);
            }

            if line.contains("error") || line.contains("Error") || line.contains("ERROR") {
                error_lines.push(line.clone());
            }

            if let Some(pb) = pb {
                if let Some(progress) = parse_ffmpeg_progress(&line) {
                    pb.set_position((progress * 1000.0) as u64);
                    if let Some(speed) = parse_ffmpeg_speed(&line) {
                        pb.set_message(format!("{}x", speed));
                    }
                }
            }
        }
    }

    Ok(())
}

fn parse_ffmpeg_progress(line: &str) -> Option<f64> {
    let time_start = line.find("time=")?;
    let time_str = &line[time_start + 5..];
    let time_end = time_str.find(' ')?;
    let time_val = &time_str[..time_end];

    parse_time_to_seconds(time_val)
}

fn parse_time_to_seconds(time_str: &str) -> Option<f64> {
    let parts: Vec<&str> = time_str.split(':').collect();
    if parts.len() != 3 {
        return None;
    }

    let hours: f64 = parts[0].parse().ok()?;
    let minutes: f64 = parts[1].parse().ok()?;
    let seconds: f64 = parts[2].parse().ok()?;

    Some(hours * 3600.0 + minutes * 60.0 + seconds)
}

fn parse_ffmpeg_speed(line: &str) -> Option<String> {
    let speed_start = line.find("speed=")?;
    let speed_str = &line[speed_start + 6..];
    let speed_end = speed_str.find('x')?;
    let speed_val = &speed_str[..speed_end + 1];
    Some(speed_val.to_string())
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
