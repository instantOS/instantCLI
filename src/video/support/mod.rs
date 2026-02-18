pub mod ffmpeg;
pub mod music;
pub mod transcript;
pub mod utils;

/// Uvx arguments for running WhisperX with compatible Python and torch versions.
/// Prevents torchaudio/torchcodec compatibility issues (see: https://github.com/m-bain/whisperX/issues/1264)
pub const WHISPERX_UVX_ARGS: &[&str] = &[
    "--python",
    "3.10",
    "--with",
    "torch<2.1",
    "--with",
    "torchaudio<2.1",
];
