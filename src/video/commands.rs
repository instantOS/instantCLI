use anyhow::Result;

use super::auphonic;
use super::check;
use super::cli::VideoCommands;
use super::convert;
use super::render;
use super::setup;
use super::stats;
use super::titlecard;
use super::transcribe;

pub async fn handle_video_command(command: VideoCommands, _debug: bool) -> Result<()> {
    match command {
        VideoCommands::Convert(args) => convert::handle_convert(args).await,
        VideoCommands::Transcribe(args) => transcribe::handle_transcribe(args),
        VideoCommands::Render(args) => render::handle_render(args),
        VideoCommands::Titlecard(args) => titlecard::cli::handle_titlecard(args),
        VideoCommands::Check(args) => check::handle_check(args),
        VideoCommands::Stats(args) => stats::handle_stats(args),
        VideoCommands::Auphonic(args) => auphonic::handle_auphonic(args).await,
        VideoCommands::Setup(args) => setup::handle_setup(args).await,
    }
}
