//! ClearVoice (Alibaba DAMO) AI speech enhancement backend.
//!
//! This is the default enhancer: fullband 48 kHz speech enhancement
//! (denoise + restoration) using MossFormer2_SE_48K, followed by the same
//! boundary-safe podcast mastering step as the Local backend.
//!
//! The heavy Python stack (torch + clearvoice) runs via `uv run --with`,
//! matching the Granite/whisperx driver pattern. Model checkpoints are
//! auto-downloaded from Hugging Face into `<models>/checkpoints` on first use.
//!
//! Pipeline (voice stem, before music is mixed):
//! 1. Extract audio to WAV if input is video
//! 2. ClearVoice AI enhancement (denoise + restoration)
//! 3. Podcast EQ, distance leveling, compression, and limiting

use std::io::{IsTerminal, Read};
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};

use super::EnhanceCache;
use super::types::{AudioEnhancer, EnhanceResult};
use crate::ui::prelude::{Level, OutputFormat, emit, get_output_format};
use crate::video::config::VideoDirectories;
use crate::video::support::ffmpeg::{AudioExtractSpec, extract_audio};
use crate::video::support::utils::is_audio_file;

/// Speech enhancement model name (fullband 48 kHz, best quality).
const CLEARVOICE_MODEL: &str = "MossFormer2_SE_48K";
const CLEARVOICE_PACKAGE: &str = "clearvoice==0.1.2";
const CLEARVOICE_DRIVER: &str = include_str!("clearvoice_driver.py");
const CLEARVOICE_INPUT_RECIPE: &str = "mono:s16:48000:highpass=80Hz:2pole";
const CLEARVOICE_INPUT_SPEC: AudioExtractSpec = AudioExtractSpec {
    codec: "pcm_s16le",
    sample_rate: 48_000,
    mono: true,
    extra_args: &[("-af", "highpass=f=80:p=2")],
    label: "DC-filtered voice audio",
};

/// Driver progress marker, mirroring Granite's `TC_PROGRESS` convention.
const CV_PROGRESS_PREFIX: &str = "CV_PROGRESS ";
/// How many captured output lines to keep for error reporting.
const OUTPUT_TAIL_LINES: usize = 24;

fn cache_tag(parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hex::encode(hasher.finalize())[..12].to_string()
}

fn driver_cache_tag() -> String {
    cache_tag(&[
        CLEARVOICE_INPUT_RECIPE.as_bytes(),
        CLEARVOICE_PACKAGE.as_bytes(),
        CLEARVOICE_DRIVER.as_bytes(),
    ])
}

fn enhanced_cache_tag() -> String {
    cache_tag(&[
        CLEARVOICE_DRIVER.as_bytes(),
        CLEARVOICE_PACKAGE.as_bytes(),
        CLEARVOICE_INPUT_RECIPE.as_bytes(),
        super::VOICE_MASTERING_RECIPE.as_bytes(),
    ])
}

/// Shared display for the driver's progress markers and its diagnostic output.
struct ClearVoiceProgress {
    bar: Option<ProgressBar>,
    bar_style: ProgressStyle,
    last_percent: Option<u64>,
    last_logged_percent: Option<u64>,
}

impl ClearVoiceProgress {
    fn new() -> Result<Self> {
        let bar_style = ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {percent:>3}% ({eta}) {msg}",
        )?
        .progress_chars("█▉▊▋▌▍▎▏ ");
        let interactive = get_output_format() == OutputFormat::Text
            && std::io::stdout().is_terminal()
            && std::io::stderr().is_terminal()
            && std::env::var("TERM").is_ok_and(|term| term != "dumb");
        let bar = if interactive {
            let bar = ProgressBar::new_spinner();
            bar.set_style(ProgressStyle::with_template(
                "{spinner:.green} [{elapsed_precise}] Preparing ClearVoice enhancement…",
            )?);
            bar.enable_steady_tick(Duration::from_millis(100));
            Some(bar)
        } else {
            None
        };
        Ok(Self {
            bar,
            bar_style,
            last_percent: None,
            last_logged_percent: None,
        })
    }

    fn update(&mut self, done_ms: u64, total_ms: u64) {
        if total_ms == 0 {
            return;
        }
        let done_ms = done_ms.min(total_ms);
        let percent = ((u128::from(done_ms) * 100) / u128::from(total_ms)) as u64;
        // Chunks that round to the same percentage should not redraw or spam.
        if self
            .last_percent
            .is_some_and(|previous| percent <= previous)
        {
            return;
        }
        let first = self.last_percent.is_none();
        self.last_percent = Some(percent);
        if let Some(bar) = &self.bar {
            if first {
                bar.set_style(self.bar_style.clone());
            }
            bar.set_length(total_ms);
            bar.set_position(done_ms);
            bar.set_message(if done_ms == 0 {
                "loading model"
            } else {
                "enhancing speech"
            });
        } else if get_output_format() == OutputFormat::Json
            || self
                .last_logged_percent
                .is_none_or(|previous| percent >= previous + 5)
            || percent == 100
        {
            self.last_logged_percent = Some(percent);
            emit(
                Level::Info,
                "video.enhance.clearvoice.progress",
                &format!("Enhancing speech with ClearVoice... {percent}%"),
                Some(serde_json::json!({
                    "done_ms": done_ms,
                    "total_ms": total_ms,
                    "percent": percent,
                })),
            );
        }
    }

    fn warning(&self, line: &str) {
        let show = || emit(Level::Warn, "video.enhance.clearvoice.warning", line, None);
        if let Some(bar) = &self.bar {
            bar.suspend(show);
        } else {
            show();
        }
    }

    fn finish(&self, success: bool) {
        if let Some(bar) = &self.bar {
            if success {
                bar.finish_with_message("done");
                // The next status line is printed outside the bar.
                eprintln!();
            } else {
                bar.finish_and_clear();
            }
        }
    }
}

fn parse_progress(line: &str) -> Option<(u64, u64)> {
    let rest = line.strip_prefix(CV_PROGRESS_PREFIX)?;
    let mut parts = rest.split_whitespace();
    let done = parts.next()?.parse().ok()?;
    let total = parts.next()?.parse().ok()?;
    Some((done, total))
}

fn is_diagnostic_warning(line: &str) -> bool {
    let lowercase = line.to_ascii_lowercase();
    lowercase.contains("warning") || lowercase.contains("error")
}

/// Drain the driver's stderr: translate progress markers and keep a rolling
/// tail of everything else for error reporting.
fn spawn_driver_reader(
    stream: impl Read + Send + 'static,
    progress: Arc<Mutex<ClearVoiceProgress>>,
) -> thread::JoinHandle<Vec<String>> {
    thread::spawn(move || {
        let mut reader = stream;
        let mut buffer = [0u8; 4096];
        let mut accumulated = String::new();
        let mut tail: Vec<String> = Vec::new();

        loop {
            let bytes_read = match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(bytes_read) => bytes_read,
                Err(_) => break,
            };
            accumulated.push_str(&String::from_utf8_lossy(&buffer[..bytes_read]));

            while let Some(position) = accumulated.find(['\r', '\n']) {
                let line = accumulated[..position].trim().to_string();
                accumulated.drain(..=position);
                if line.is_empty() {
                    continue;
                }
                handle_driver_line(&line, &progress, &mut tail);
            }
        }

        let line = accumulated.trim().to_string();
        if !line.is_empty() {
            handle_driver_line(&line, &progress, &mut tail);
        }
        tail
    })
}

fn handle_driver_line(
    line: &str,
    progress: &Arc<Mutex<ClearVoiceProgress>>,
    tail: &mut Vec<String>,
) {
    if let Some((done, total)) = parse_progress(line) {
        if let Ok(mut progress) = progress.lock() {
            progress.update(done, total);
        }
        return;
    }

    if is_diagnostic_warning(line)
        && let Ok(progress) = progress.lock()
    {
        progress.warning(line);
    }

    if tail.len() >= OUTPUT_TAIL_LINES {
        tail.remove(0);
    }
    tail.push(line.to_string());
}

/// ClearVoice enhancer driven by `uv run --with clearvoice`.
pub struct ClearVoiceEnhancer;

impl ClearVoiceEnhancer {
    pub fn new() -> Self {
        Self
    }

    /// Run the embedded driver: AI enhancement, cwd = models dir so
    /// `./checkpoints/...` (auto-downloaded once) lands next to the venv.
    fn run_driver(input: &Path, output: &Path) -> Result<()> {
        emit(
            Level::Info,
            "video.enhance.clearvoice.model",
            &format!("Running ClearVoice {} enhancement...", CLEARVOICE_MODEL),
            None,
        );

        let directories = VideoDirectories::new()?;
        let models_dir = directories.models_dir();
        std::fs::create_dir_all(&models_dir)?;

        let driver_path = models_dir.join("clearvoice_driver.py");
        let driver_is_current = std::fs::read_to_string(&driver_path)
            .map(|contents| contents == CLEARVOICE_DRIVER)
            .unwrap_or(false);
        if !driver_is_current {
            std::fs::write(&driver_path, CLEARVOICE_DRIVER)?;
        }

        let mut child = std::process::Command::new("uv")
            .args(["run", "--with", CLEARVOICE_PACKAGE, "python"])
            .arg(&driver_path)
            .arg(input)
            .arg(output)
            .current_dir(&models_dir)
            .env_remove("MPLBACKEND")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| {
                format!(
                    "Failed to run ClearVoice driver at {}",
                    driver_path.display()
                )
            })?;

        let stdout = child
            .stdout
            .take()
            .context("Failed to capture ClearVoice driver stdout")?;
        let stderr = child
            .stderr
            .take()
            .context("Failed to capture ClearVoice driver stderr")?;
        let progress = Arc::new(Mutex::new(ClearVoiceProgress::new()?));
        let stdout_reader = spawn_driver_reader(stdout, Arc::clone(&progress));
        let stderr_reader = spawn_driver_reader(stderr, Arc::clone(&progress));
        let status = child
            .wait()
            .context("Failed to wait for ClearVoice driver")?;
        let mut tail = stderr_reader
            .join()
            .map_err(|_| anyhow::anyhow!("ClearVoice stderr reader thread panicked"))?;
        tail.extend(
            stdout_reader
                .join()
                .map_err(|_| anyhow::anyhow!("ClearVoice stdout reader thread panicked"))?,
        );

        if let Ok(progress) = progress.lock() {
            progress.finish(status.success());
        }

        if !status.success() {
            let details = tail.join("\n");
            anyhow::bail!(
                "ClearVoice failed to enhance {}:\n{}",
                input.display(),
                details.trim()
            );
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl AudioEnhancer for ClearVoiceEnhancer {
    async fn enhance(&self, input: &Path, force: bool) -> Result<EnhanceResult> {
        let cache = EnhanceCache::prepare(input)?;

        // Enhanced stem for the render mix. Engine-tagged so switching
        // backends never serves a stale result produced by another engine.
        let enhanced_cache_path =
            cache.path(&format!("enhanced_clearvoice_{}.wav", enhanced_cache_tag()));

        // Check cache
        if let Some(cached) = cache.cached(&enhanced_cache_path, force, "video.enhance.cached", "")
        {
            return Ok(cached);
        }

        // Step 1: Get audio as WAV
        let wav_path = cache.path(&format!(
            "extracted_clearvoice_{}.wav",
            cache_tag(&[CLEARVOICE_INPUT_RECIPE.as_bytes()])
        ));
        if !wav_path.exists() || force {
            if !is_audio_file(input) {
                emit(
                    Level::Info,
                    "video.enhance.extract",
                    &format!("Extracting audio from {}...", input.display()),
                    None,
                );
            }
            extract_audio(input, &wav_path, &CLEARVOICE_INPUT_SPEC)?;
        }

        // Step 2: ClearVoice AI enhancement (denoise + restoration)
        let cv_raw_path = cache.path(&format!("clearvoice_raw_{}.wav", driver_cache_tag()));
        if !cv_raw_path.exists() || force {
            Self::run_driver(&wav_path, &cv_raw_path)?;
        }

        // Step 3: Stable podcast mastering for consistent mic levels.
        super::run_voice_mastering(&cv_raw_path, &enhanced_cache_path)?;

        Ok(EnhanceResult {
            output_path: enhanced_cache_path,
        })
    }

    fn name(&self) -> &'static str {
        "clearvoice"
    }

    fn is_available(&self) -> bool {
        which::which("uv").is_ok() && which::which("ffmpeg").is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hidden_progress() -> Arc<Mutex<ClearVoiceProgress>> {
        Arc::new(Mutex::new(ClearVoiceProgress {
            bar: None,
            bar_style: ProgressStyle::with_template("{bar}").unwrap(),
            last_percent: None,
            last_logged_percent: None,
        }))
    }

    #[test]
    fn parses_driver_progress_markers() {
        assert_eq!(parse_progress("CV_PROGRESS 0 1000"), Some((0, 1000)));
        assert_eq!(parse_progress("CV_PROGRESS 250 1000"), Some((250, 1000)));
        assert_eq!(parse_progress("CV_PROGRESS 250"), None);
        assert_eq!(parse_progress("CV_PROGRESS nope 1000"), None);
        assert_eq!(parse_progress("ClearVoice chunk 1"), None);
    }

    #[test]
    fn driver_reader_translates_progress_and_keeps_tail() {
        let progress = hidden_progress();
        let tail = spawn_driver_reader(
            std::io::Cursor::new(b"CV_PROGRESS 500 1000\r\nCV_PROGRESS 500 1000\nhelper output\n"),
            Arc::clone(&progress),
        )
        .join()
        .expect("reader thread");

        assert_eq!(tail, vec!["helper output".to_string()]);
        assert_eq!(progress.lock().unwrap().last_percent, Some(50));
    }
}
