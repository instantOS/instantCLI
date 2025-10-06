use clap::{Args, Subcommand, ValueHint};
use std::path::PathBuf;

#[derive(Subcommand, Debug, Clone)]
pub enum VideoCommands {
    /// Convert a timestamped transcript into editable video markdown
    Convert(ConvertArgs),
}

#[derive(Args, Debug, Clone)]
pub struct ConvertArgs {
    /// Source video file
    #[arg(value_hint = ValueHint::FilePath)]
    pub video: PathBuf,

    /// Timestamped transcript file (currently SRT)
    #[arg(short = 't', long = "transcript", value_hint = ValueHint::FilePath)]
    pub transcript: PathBuf,

    /// Optional output file path; defaults to the project markdown file
    #[arg(short, long, value_hint = ValueHint::FilePath)]
    pub output: Option<PathBuf>,
}
