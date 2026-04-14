pub mod ffmpeg;
pub mod music;
pub mod transcript;
pub mod utils;

/// Uvx arguments for running WhisperX with compatible Python version.
pub const WHISPERX_UVX_ARGS: &[&str] = &["--python", "3.10"];
