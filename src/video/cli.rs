use clap::{Args, Subcommand, ValueHint};
use std::path::PathBuf;

#[derive(Subcommand, Debug, Clone)]
pub enum VideoCommands {
    /// Convert a timestamped transcript into editable video markdown
    Convert(ConvertArgs),
    /// Append another recording to a video markdown file
    Append(AppendArgs),
    /// Generate a transcript for a video using WhisperX
    Transcribe(TranscribeArgs),
    /// Render a video according to edits in a markdown file
    Render(RenderArgs),
    /// Preview the video with ffplay (allows scrubbing with arrow keys)
    Preview(PreviewArgs),
    /// Generate a slide image from a markdown file
    Slide(SlideArgs),
    /// Validate and show statistics for a video markdown file
    Check(CheckArgs),
    /// Process audio with the configured preprocessor (local or auphonic)
    Preprocess(PreprocessArgs),
    /// Setup video tools (local preprocessor, Auphonic, WhisperX)
    Setup(SetupArgs),
    /// Interactive video menu (guided workflows)
    Menu,
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
pub struct AppendArgs {
    /// Existing markdown file to append to
    #[arg(value_hint = ValueHint::FilePath)]
    pub markdown: PathBuf,

    /// Source video file to append
    #[arg(value_hint = ValueHint::FilePath)]
    pub video: PathBuf,

    /// Timestamped transcript file (WhisperX JSON)
    #[arg(short = 't', long = "transcript", value_hint = ValueHint::FilePath)]
    pub transcript: Option<PathBuf>,

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

    /// Show the ffmpeg command that would be executed without running it
    #[arg(long)]
    pub dry_run: bool,

    #[command(flatten)]
    pub common: VideoProcessArgs,
}

#[derive(Args, Debug, Clone)]
pub struct PreviewArgs {
    /// Markdown file describing the edited timeline
    #[arg(value_hint = ValueHint::FilePath)]
    pub markdown: PathBuf,

    #[command(flatten)]
    pub common: VideoProcessArgs,

    /// Seek to specific time in seconds before starting preview
    #[arg(long, value_name = "SECONDS")]
    pub seek: Option<f64>,
}

/// Common arguments for video processing commands (render, preview)
#[derive(Args, Debug, Clone)]
pub struct VideoProcessArgs {
    /// Pre-cache slides without processing
    #[arg(long = "precache-slides")]
    pub precache_slides: bool,

    /// Process in Instagram Reels/TikTok format (9:16 vertical)
    #[arg(long)]
    pub reels: bool,

    /// Burn subtitles into the output (works in both normal and reels mode)
    #[arg(long)]
    pub subtitles: bool,

    /// Show raw output instead of progress bar
    #[arg(long)]
    pub verbose: bool,
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
