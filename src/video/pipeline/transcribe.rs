use anyhow::{Context, Result, bail};
use duct::cmd;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

use crate::ui::prelude::{Level, emit};

use crate::video::cli::{TranscribeArgs, TranscribeBackend};
use crate::video::config::VideoDirectories;
use crate::video::support::WHISPERX_UVX_ARGS;
use crate::video::support::ffmpeg::{AudioExtractSpec, extract_audio};
use crate::video::support::transcript::parse_whisper_json;
use crate::video::support::utils::{
    canonicalize_existing, compute_file_hash, copy_overwriting, extension_or_default,
};

/// Granite Speech driver script, run via `uv run --with transcribe-cpp`.
const GRANITE_DRIVER: &str = include_str!("granite_driver.py");

/// Default Granite model: IBM Granite Speech 4.1-2b-plus, Q8_0 (2.35 GB, Apache-2.0).
const GRANITE_MODEL_URL: &str = "https://huggingface.co/handy-computer/granite-speech-4.1-2b-plus-gguf/resolve/main/granite-speech-4.1-2b-plus-Q8_0.gguf";
const GRANITE_MODEL_FILENAME: &str = "granite-speech-4.1-2b-plus-Q8_0.gguf";

/// Python version requested from uv for the driver environment.
const GRANITE_DRIVER_PYTHON: &str = "3.12";

/// Granite word-timestamp windows are capped at ~3.5 min of audio; 200 s
/// keeps a comfortable margin while limiting per-window memory.
const GRANITE_WINDOW_MS: u64 = 200_000;

/// How many captured output lines to keep for error reporting.
const OUTPUT_TAIL_LINES: usize = 24;

pub fn handle_transcribe(args: TranscribeArgs) -> Result<()> {
    let video_path = canonicalize_existing(&args.video)?;
    emit(
        Level::Info,
        "video.transcribe.start",
        &format!("Starting transcription for {}...", video_path.display()),
        None,
    );
    let video_hash = compute_file_hash(&video_path)?;

    let directories = VideoDirectories::new()?;
    let cache_paths = directories.cache_paths(&video_hash, args.language);
    cache_paths.ensure_directories()?;

    let transcript_path = cache_paths.transcript_cache_path().to_path_buf();
    if transcript_path.exists() && !args.force {
        emit(
            Level::Info,
            "video.transcribe.cached",
            &format!(
                "Transcript already cached at {} (use --force to regenerate)",
                transcript_path.display()
            ),
            None,
        );
        return Ok(());
    }

    let extension = extension_or_default(&video_path, "mp4");
    let hashed_video_path = cache_paths.hashed_video_input(&extension);
    copy_overwriting(&video_path, &hashed_video_path, "temporary video")?;

    let models_dir = directories.models_dir();
    let run_result = match resolve_backend(&args, &models_dir)? {
        BackendChoice::Granite => {
            emit(
                Level::Info,
                "video.transcribe.backend",
                "Using Granite (transcribe.cpp)",
                None,
            );
            run_granite(&hashed_video_path, &transcript_path, &models_dir, &args)
        }
        BackendChoice::WhisperX => {
            emit(
                Level::Info,
                "video.transcribe.backend",
                "Using WhisperX",
                None,
            );
            run_whisperx(&hashed_video_path, cache_paths.transcript_dir(), &args)
                .and_then(|()| relocate_whisperx_output(&hashed_video_path, &transcript_path))
        }
    };

    // Clean up temporary copy regardless of success
    if let Err(err) = cleanup_hashed_video_input(&hashed_video_path) {
        emit(
            Level::Warn,
            "video.transcribe.cleanup_failed",
            &format!(
                "Failed to remove temporary file {}: {}",
                hashed_video_path.display(),
                err
            ),
            None,
        );
    }

    run_result?;

    if !transcript_path.exists() {
        anyhow::bail!(
            "Transcription did not produce the expected transcript at {}",
            transcript_path.display()
        );
    }

    emit(
        Level::Success,
        "video.transcribe.success",
        &format!("Generated transcript at {}", transcript_path.display()),
        None,
    );

    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BackendChoice {
    Granite,
    WhisperX,
}

/// Resolve the effective backend. `auto` prefers Granite only when its model
/// is already installed, falling back to WhisperX without triggering the
/// Resolve which transcription backend to use. `Auto` and `Granite` both
/// resolve to Granite; the model is downloaded on first use via
/// `ensure_granite_model`.
fn resolve_backend(args: &TranscribeArgs, _models_dir: &Path) -> Result<BackendChoice> {
    match args.backend {
        TranscribeBackend::Granite | TranscribeBackend::Auto => Ok(BackendChoice::Granite),
        TranscribeBackend::Whisperx => Ok(BackendChoice::WhisperX),
    }
}

/// The Granite GGUF path: `--granite-model` local file or download target, or
/// the default model under the shared models directory.
fn granite_model_path(args: &TranscribeArgs, models_dir: &Path) -> Result<PathBuf> {
    if let Some(spec) = &args.granite_model {
        let local = Path::new(spec);
        if local.is_file() {
            return Ok(local.to_path_buf());
        }
        if spec.starts_with("http://") || spec.starts_with("https://") {
            let name = spec
                .rsplit(['/', '?'])
                .find(|part| !part.is_empty())
                .unwrap_or(GRANITE_MODEL_FILENAME);
            return Ok(models_dir.join(name));
        }
        bail!("--granite-model must be an existing file or an http(s) URL (got {spec:?})");
    }
    Ok(models_dir.join(GRANITE_MODEL_FILENAME))
}

/// Ensure the Granite GGUF exists, downloading it on demand.
fn ensure_granite_model(args: &TranscribeArgs, models_dir: &Path) -> Result<PathBuf> {
    let target = granite_model_path(args, models_dir)?;
    if target.exists() {
        return Ok(target);
    }

    let url = if let Some(spec) = &args.granite_model {
        if spec.starts_with("http://") || spec.starts_with("https://") {
            spec.clone()
        } else {
            GRANITE_MODEL_URL.to_string()
        }
    } else {
        GRANITE_MODEL_URL.to_string()
    };

    emit(
        Level::Info,
        "video.transcribe.model_download",
        &format!(
            "Downloading Granite model to {} (one-time, ~2.4 GB)...",
            target.display()
        ),
        None,
    );
    download_file(&url, &target, "Granite model")?;
    Ok(target)
}

/// Stream `url` to `dest` atomically via a `.part` temp file, emitting
/// throttled progress events when the server provides Content-Length.
///
/// Runs on a dedicated thread: `reqwest::blocking` cannot be created or
/// dropped inside an async runtime (ins runs under tokio), but a fresh OS
/// thread has no runtime context, so the blocking client is safe there.
fn download_file(url: &str, dest: &Path, what: &str) -> Result<()> {
    let url = url.to_string();
    let dest = dest.to_path_buf();
    let progress_what = what.to_string();
    let worker_what = what.to_string();
    let (tx, rx) = std::sync::mpsc::channel::<u64>();

    let worker = thread::spawn(move || -> Result<(), String> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("ins/0.1")
            .build()
            .map_err(|e| format!("Failed to build HTTP client: {e}"))?;
        let mut response = client
            .get(&url)
            .send()
            .map_err(|e| format!("Failed to start download of {worker_what} from {url}: {e}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "Download of {worker_what} failed: HTTP {}",
                response.status()
            ));
        }

        let parent = dest
            .parent()
            .ok_or_else(|| format!("Model path {} has no parent directory", dest.display()))?;
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create model directory {}: {e}", parent.display()))?;

        let file_name = dest
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "model".to_string());
        let tmp = parent.join(format!(".{file_name}.part"));
        let mut file = fs::File::create(&tmp).map_err(|e| {
            format!(
                "Failed to create temporary download file {}: {e}",
                tmp.display()
            )
        })?;

        let total = response.content_length().unwrap_or(0);
        let mut done: u64 = 0;
        let mut last_emitted: u64 = 0;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let n = response
                .read(&mut buffer)
                .map_err(|e| format!("Failed while downloading {worker_what}: {e}"))?;
            if n == 0 {
                break;
            }
            file.write_all(&buffer[..n])
                .map_err(|e| format!("Failed while writing downloaded {worker_what}: {e}"))?;
            done += n as u64;
            let pct = (done * 100).checked_div(total).unwrap_or(0);
            if pct >= last_emitted + 5 && tx.send(pct).is_err() {
                break; // caller went away
            }
            last_emitted = pct;
        }
        if total > 0 && done != total {
            let _ = fs::remove_file(&tmp);
            return Err(format!(
                "Download of {worker_what} was truncated ({done} of {total} bytes)"
            ));
        }
        fs::rename(&tmp, &dest).map_err(|e| {
            format!(
                "Failed to move downloaded {worker_what} into place at {}: {e}",
                dest.display()
            )
        })?;
        Ok(())
    });

    for pct in rx {
        emit(
            Level::Info,
            "video.transcribe.model_download",
            &format!("Downloading {progress_what}... {pct}%"),
            None,
        );
    }
    worker
        .join()
        .map_err(|_| anyhow::anyhow!("download worker thread panicked"))?
        .map_err(anyhow::Error::msg)
}

fn run_granite(
    hashed_video: &Path,
    transcript_path: &Path,
    models_dir: &Path,
    args: &TranscribeArgs,
) -> Result<()> {
    fs::create_dir_all(models_dir)
        .with_context(|| format!("Failed to create model directory {}", models_dir.display()))?;

    let model_path = ensure_granite_model(args, models_dir)?;

    // Materialize the driver script next to the model.
    let driver_path = models_dir.join("granite_driver.py");
    if !driver_path.exists() {
        fs::write(&driver_path, GRANITE_DRIVER)
            .with_context(|| format!("Failed to write driver script {}", driver_path.display()))?;
    }

    // 16 kHz mono WAV is the transcribe.cpp input format; use a distinct name
    // so WAV sources (whose hashed copy already ends in `.wav`) don't collide
    // with the extraction target. Cleaned up after the run.
    let wav_path = hashed_video.with_file_name(format!(
        "{}_16k.wav",
        hashed_video
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "audio".to_string())
    ));
    extract_audio(hashed_video, &wav_path, &AudioExtractSpec::MONO_16K_WAV)?;

    let language = args.language.whisper_code();
    let result = run_granite_driver(
        &driver_path,
        &model_path,
        &wav_path,
        transcript_path,
        language,
    );

    // The WAV is a temporary artifact; remove it even on failure (best effort).
    if let Err(err) = fs::remove_file(&wav_path)
        && err.kind() != std::io::ErrorKind::NotFound
    {
        emit(
            Level::Warn,
            "video.transcribe.cleanup_failed",
            &format!(
                "Failed to remove temporary audio {}: {}",
                wav_path.display(),
                err
            ),
            None,
        );
    }

    result?;

    // Sanity-check that the driver emitted parseable transcript data.
    let contents = fs::read_to_string(transcript_path).with_context(|| {
        format!(
            "Failed to read Granite transcript at {}",
            transcript_path.display()
        )
    })?;
    let cues = parse_whisper_json(&contents)?;
    if cues.is_empty() {
        bail!(
            "Granite produced no transcript cues at {}",
            transcript_path.display()
        );
    }
    Ok(())
}

fn run_granite_driver(
    driver: &Path,
    model: &Path,
    wav: &Path,
    out: &Path,
    language: &str,
) -> Result<()> {
    let mut command = Command::new("uv");
    command
        .args([
            "run",
            "--no-project",
            "--python",
            GRANITE_DRIVER_PYTHON,
            "--with",
            "transcribe-cpp",
            "--with",
            "numpy",
        ])
        .arg("python")
        .arg(driver)
        .arg("--model")
        .arg(model)
        .arg("--wav")
        .arg(wav)
        .arg("--out")
        .arg(out)
        .arg("--language")
        .arg(language)
        .arg("--window-ms")
        .arg(GRANITE_WINDOW_MS.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .context("Failed to spawn `uv run` for transcription")?;
    let stdout = child
        .stdout
        .take()
        .context("Failed to capture driver stdout")?;
    let stderr = child
        .stderr
        .take()
        .context("Failed to capture driver stderr")?;

    let stdout_reader = spawn_output_reader(stdout);
    let stderr_reader = spawn_output_reader(stderr);

    let status = child.wait().context("Failed to wait for transcription")?;
    let stdout_lines = stdout_reader
        .join()
        .map_err(|_| anyhow::anyhow!("stdout reader thread panicked"))?;
    let stderr_lines = stderr_reader
        .join()
        .map_err(|_| anyhow::anyhow!("stderr reader thread panicked"))?;

    if !status.success() {
        bail!(
            "Granite transcription failed (exit {status}):\n{}",
            format_tail(&stderr_lines)
        );
    }
    let _ = stdout_lines;
    Ok(())
}

fn format_tail(lines: &[String]) -> String {
    lines.join("\n").trim().to_string()
}

/// Drain one output stream: translate `TC_PROGRESS <done> <total>` lines into
/// progress events and keep a rolling tail of everything else for errors.
fn spawn_output_reader(stream: impl Read + Send + 'static) -> thread::JoinHandle<Vec<String>> {
    thread::spawn(move || {
        let reader = BufReader::new(stream);
        let mut tail: Vec<String> = Vec::new();
        for line in reader.lines().map_while(Result::ok) {
            let line = line.trim().to_string();
            if let Some(rest) = line.strip_prefix("TC_PROGRESS ") {
                if let Some((done, total)) = rest.split_once(' ')
                    && let (Ok(done), Ok(total)) = (done.parse::<u64>(), total.parse::<u64>())
                {
                    let pct = (done * 100).checked_div(total).unwrap_or(100);
                    emit(
                        Level::Info,
                        "video.transcribe.progress",
                        &format!("Transcribing... {pct}%"),
                        None,
                    );
                }
                continue;
            }
            if tail.len() >= OUTPUT_TAIL_LINES {
                tail.remove(0);
            }
            tail.push(line);
        }
        tail
    })
}

fn run_whisperx(hashed_video: &Path, output_dir: &Path, args: &TranscribeArgs) -> Result<()> {
    let hashed_video = hashed_video.to_string_lossy();
    let output_dir = output_dir.to_string_lossy();

    let language = args.language.whisper_code();
    let align_model = args.language.align_model();

    let mut whisper_args: Vec<&str> = vec![
        "whisperx",
        &hashed_video,
        "--language",
        language,
        "--output_format",
        "json",
        "--output_dir",
        &output_dir,
        "--vad_method",
        &args.vad_method,
        "--compute_type",
        &args.compute_type,
        "--device",
        &args.device,
        "--align_model",
        align_model,
        "--batch_size",
        "4",
        "--segment_resolution",
        "sentence",
        "--beam_size",
        "5",
        "--patience",
        "1.0",
        "--max_line_width",
        "42",
        "--threads",
        "8",
    ];

    if let Some(model) = &args.model {
        whisper_args.push("--model");
        whisper_args.push(model);
    }

    // Combine uvx base args with whisperx-specific args
    let mut full_args: Vec<&str> = WHISPERX_UVX_ARGS.to_vec();
    full_args.extend(whisper_args);

    cmd("uvx", &full_args)
        .run()
        .with_context(|| format!("Failed to run WhisperX for {}", hashed_video))?;

    Ok(())
}

/// WhisperX writes `{input_stem}.json`; German (and other non-default names)
/// differ from the canonical cache filename. Move it into place when needed.
fn relocate_whisperx_output(hashed_video: &Path, transcript_path: &Path) -> Result<()> {
    let whisper_json = hashed_video.with_extension("json");
    if whisper_json != transcript_path && whisper_json.exists() {
        if transcript_path.exists() {
            fs::remove_file(transcript_path).with_context(|| {
                format!(
                    "Failed to remove stale transcript at {}",
                    transcript_path.display()
                )
            })?;
        }
        fs::rename(&whisper_json, transcript_path).with_context(|| {
            format!(
                "Failed to move WhisperX output {} to {}",
                whisper_json.display(),
                transcript_path.display()
            )
        })?;
    }
    Ok(())
}

fn cleanup_hashed_video_input(hashed_path: &Path) -> Result<()> {
    if hashed_path.exists() {
        fs::remove_file(hashed_path).with_context(|| {
            format!("Failed to remove temporary file {}", hashed_path.display())
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_models(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ins-tc-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn args_with(backend: TranscribeBackend) -> TranscribeArgs {
        TranscribeArgs {
            video: PathBuf::from("x.mp4"),
            compute_type: "int8".into(),
            device: "cpu".into(),
            model: None,
            vad_method: "silero".into(),
            force: false,
            backend,
            granite_model: None,
            language: crate::video::transcript_language::TranscriptLanguage::En,
        }
    }

    #[test]
    fn resolve_backend_auto_uses_granite() {
        let dir = temp_models("auto");
        let choice = resolve_backend(&args_with(TranscribeBackend::Auto), &dir).unwrap();
        assert_eq!(choice, BackendChoice::Granite);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resolve_backend_auto_prefers_installed_granite() {
        let dir = temp_models("auto2");
        fs::write(dir.join(GRANITE_MODEL_FILENAME), b"gguf").unwrap();
        let choice = resolve_backend(&args_with(TranscribeBackend::Auto), &dir).unwrap();
        assert_eq!(choice, BackendChoice::Granite);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resolve_backend_explicit_wins() {
        let dir = temp_models("explicit");
        assert_eq!(
            resolve_backend(&args_with(TranscribeBackend::Granite), &dir).unwrap(),
            BackendChoice::Granite
        );
        assert_eq!(
            resolve_backend(&args_with(TranscribeBackend::Whisperx), &dir).unwrap(),
            BackendChoice::WhisperX
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn granite_model_path_defaults_under_models_dir() {
        let dir = temp_models("path");
        let p = granite_model_path(&args_with(TranscribeBackend::Auto), &dir).unwrap();
        assert!(p.ends_with(GRANITE_MODEL_FILENAME));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn granite_model_path_accepts_local_file() {
        let dir = temp_models("local");
        let gguf = dir.join("custom.gguf");
        fs::write(&gguf, b"x").unwrap();
        let mut args = args_with(TranscribeBackend::Granite);
        args.granite_model = Some(gguf.to_string_lossy().to_string());
        let p = granite_model_path(&args, &dir).unwrap();
        assert_eq!(p, gguf);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn granite_model_path_rejects_invalid_spec() {
        let dir = temp_models("bad");
        let mut args = args_with(TranscribeBackend::Granite);
        args.granite_model = Some("not-a-file".into());
        assert!(granite_model_path(&args, &dir).is_err());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn granite_driver_output_parses_like_whisperx() {
        // The exact shape the embedded driver emits must feed parse_whisper_json.
        let json = r#"{
            "segments": [
                {
                    "start": 0.0, "end": 1.1,
                    "text": "Hallo Welt",
                    "words": [
                        {"word": "Hallo", "start": 0.0, "end": 0.4},
                        {"word": "Welt", "start": 0.5, "end": 1.1}
                    ]
                }
            ]
        }"#;
        let cues = parse_whisper_json(json).expect("driver JSON must parse");
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].words.len(), 2);
        assert_eq!(cues[0].text, "Hallo Welt");
    }
}
