use clap::{Args, Subcommand, ValueHint};
use std::path::PathBuf;

#[derive(Subcommand, Debug, Clone)]
pub enum VideoCommands {
    /// Convert a timestamped transcript into editable video markdown
    Convert(ConvertArgs),
    /// Generate a transcript for a video using WhisperX
    Transcribe(TranscribeArgs),
    /// Render a video according to edits in a markdown file
    Render(RenderArgs),
    /// Generate a title card image from a markdown file
    Titlecard(TitlecardArgs),
    /// Display statistics about how a markdown file will be rendered
    Stats(StatsArgs),
}

#[derive(Args, Debug, Clone)]
pub struct ConvertArgs {
    /// Source video file
    #[arg(value_hint = ValueHint::FilePath)]
    pub video: PathBuf,

    /// Timestamped transcript file (currently SRT)
    #[arg(short = 't', long = "transcript", value_hint = ValueHint::FilePath)]
    pub transcript: Option<PathBuf>,

    /// Optional output file path; defaults to <videoname>.md next to the video
    #[arg(short = 'o', long = "out-file", value_hint = ValueHint::FilePath)]
    pub out_file: Option<PathBuf>,

    /// Overwrite existing markdown file
    #[arg(long)]
    pub force: bool,
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

#[derive(Args, Debug, Clone)]
pub struct RenderArgs {
    /// Markdown file describing the edited timeline
    #[arg(value_hint = ValueHint::FilePath)]
    pub markdown: PathBuf,

    /// Optional output path; defaults to <videoname>_edit.<ext>
    #[arg(short = 'o', long = "out-file", value_hint = ValueHint::FilePath)]
    pub out_file: Option<PathBuf>,

    /// Overwrite an existing output file
    #[arg(long)]
    pub force: bool,

    /// Pre-cache title cards without rendering the final video
    #[arg(long = "precache-titlecards")]
    pub precache_titlecards: bool,
}

#[derive(Args, Debug, Clone)]
pub struct TitlecardArgs {
    /// Markdown file containing the title card text
    #[arg(value_hint = ValueHint::FilePath)]
    pub markdown: PathBuf,

    /// Optional output path; defaults to <markdownfilename>.jpg
    #[arg(short = 'o', long = "out-file", value_hint = ValueHint::FilePath)]
    pub out_file: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub struct StatsArgs {
    /// Markdown file describing the edited timeline
    #[arg(value_hint = ValueHint::FilePath)]
    pub markdown: PathBuf,
}
