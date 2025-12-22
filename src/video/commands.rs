use anyhow::Result;

use super::audio_preprocessing;
use super::check;
use super::cli::VideoCommands;
use super::convert;
use super::render;
use super::setup;
use super::slide;
use super::stats;
use super::transcribe;

pub async fn handle_video_command(command: VideoCommands, _debug: bool) -> Result<()> {
    match command {
        VideoCommands::Convert(args) => convert::handle_convert(args).await,
        VideoCommands::Transcribe(args) => transcribe::handle_transcribe(args),
        VideoCommands::Render(args) => render::handle_render(args),
        VideoCommands::Slide(args) => slide::cli::handle_slide(args),
        VideoCommands::Check(args) => check::handle_check(args),
        VideoCommands::Stats(args) => stats::handle_stats(args),
        VideoCommands::Preprocess(args) => audio_preprocessing::handle_preprocess(args).await,
        VideoCommands::Setup(args) => setup::handle_setup(args).await,
    }
}
