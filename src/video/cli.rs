use clap::{Args, Subcommand, ValueHint};
use std::path::PathBuf;

#[derive(Subcommand, Debug, Clone)]
pub enum VideoCommands {
    /// Convert a timestamped transcript into editable video markdown
    Convert(ConvertArgs),
    /// Generate a transcript for a video using WhisperX
    Transcribe(TranscribeArgs),
}

#[derive(Args, Debug, Clone)]
pub struct ConvertArgs {
    /// Source video file
    #[arg(value_hint = ValueHint::FilePath)]
    pub video: PathBuf,

    /// Timestamped transcript file (currently SRT)
    #[arg(short = 't', long = "transcript", value_hint = ValueHint::FilePath)]
    pub transcript: Option<PathBuf>,

    /// Optional output file path; defaults to the project markdown file
    #[arg(short = 'o', long = "out-file", value_hint = ValueHint::FilePath)]
    pub output: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub struct TranscribeArgs {
    /// Source video or audio file to transcribe
    #[arg(value_hint = ValueHint::FilePath)]
    pub video: PathBuf,

    /// WhisperX compute type (e.g. int8, float16)
    #[arg(long, default_value = "int8")]
    pub compute_type: String,

    /// Target device for WhisperX (e.g. cpu, cuda)
    #[arg(long, default_value = "cpu")]
    pub device: String,

    /// Optional Whisper model override
    #[arg(long)]
    pub model: Option<String>,

    /// Re-generate transcript even if cached
    #[arg(long)]
    pub force: bool,
}
