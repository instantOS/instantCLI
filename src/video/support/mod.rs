pub mod ffmpeg;
pub mod music;
pub mod transcript;
pub mod utils;

/// Uvx arguments for running WhisperX with compatible Python version.
pub const WHISPERX_UVX_ARGS: &[&str] = &["--python", "3.10"];

/// Uvx arguments for running DeepFilterNet with compatible Python version.
pub const DEEPFILTER_UVX_ARGS: &[&str] = &[
    "--python",
    "3.10",
    "--from",
    "deepfilternet",
    "--with",
    "torch<2.1",
    "--with",
    "torchaudio<2.1",
    "deepFilter",
];
