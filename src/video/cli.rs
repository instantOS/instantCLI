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
    /// Generate a slide image from a markdown file
    Slide(SlideArgs),
    /// Validate a video markdown file and summarize the planned output
    Check(CheckArgs),
    /// Display statistics about how a markdown file will be rendered
    Stats(StatsArgs),
    /// Process audio with the configured preprocessor (local or auphonic)
    Preprocess(PreprocessArgs),
    /// Setup video tools (local preprocessor, Auphonic, WhisperX)
    Setup(SetupArgs),
}

#[derive(Args, Debug, Clone)]
pub struct SetupArgs {
    /// Force setup even if already configured
    #[arg(long)]
    pub force: bool,
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

    /// Skip audio preprocessing entirely
    #[arg(long)]
    pub no_preprocess: bool,

    /// Override preprocessor type (local, auphonic, none)
    #[arg(long)]
    pub preprocessor: Option<String>,
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

    /// VAD method for voice activity detection (e.g. silero, pyannote, audit)
    #[arg(long, default_value = "silero")]
    pub vad_method: String,

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

    /// Pre-cache slides without rendering the final video
    #[arg(long = "precache-slides")]
    pub precache_slides: bool,

    /// Show the ffmpeg command that would be executed without running it
    #[arg(long)]
    pub dry_run: bool,

    /// Render in Instagram Reels/TikTok format (9:16 vertical)
    #[arg(long)]
    pub reels: bool,

    /// Burn subtitles into the video (reels mode only, positions in bottom bar)
    #[arg(long)]
    pub subtitles: bool,
}

#[derive(Args, Debug, Clone)]
pub struct SlideArgs {
    /// Markdown file containing the slide text
    #[arg(value_hint = ValueHint::FilePath)]
    pub markdown: PathBuf,

    /// Optional output path; defaults to <markdownfilename>.jpg
    #[arg(short = 'o', long = "out-file", value_hint = ValueHint::FilePath)]
    pub out_file: Option<PathBuf>,

    /// Render in Instagram Reels/TikTok format (9:16 vertical)
    #[arg(long)]
    pub reels: bool,
}

#[derive(Args, Debug, Clone)]
pub struct CheckArgs {
    /// Markdown file describing the edited timeline
    #[arg(value_hint = ValueHint::FilePath)]
    pub markdown: PathBuf,
}

#[derive(Args, Debug, Clone)]
pub struct StatsArgs {
    /// Markdown file describing the edited timeline
    #[arg(value_hint = ValueHint::FilePath)]
    pub markdown: PathBuf,
}

#[derive(Args, Debug, Clone)]
pub struct PreprocessArgs {
    /// Source video or audio file to process
    #[arg(value_hint = ValueHint::FilePath)]
    pub input_file: PathBuf,

    /// Preprocessor backend: local, auphonic, none
    #[arg(long, default_value = "local")]
    pub backend: String,

    /// Force reprocessing even if cached
    #[arg(long)]
    pub force: bool,

    /// Auphonic Preset UUID (only for auphonic backend)
    #[arg(long)]
    pub preset: Option<String>,

    /// Auphonic API key (only for auphonic backend)
    #[arg(long)]
    pub api_key: Option<String>,
}
