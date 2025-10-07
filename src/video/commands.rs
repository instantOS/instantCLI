use anyhow::Result;

use super::cli::VideoCommands;
use super::convert::handle_convert;
use super::render::handle_render;
use super::stats::handle_stats;
use super::titlecard::handle_titlecard;
use super::transcribe::handle_transcribe;

pub fn handle_video_command(command: VideoCommands, _debug: bool) -> Result<()> {
    match command {
        VideoCommands::Convert(args) => handle_convert(args),
        VideoCommands::Transcribe(args) => handle_transcribe(args),
        VideoCommands::Render(args) => handle_render(args),
        VideoCommands::Titlecard(args) => handle_titlecard(args),
        VideoCommands::Stats(args) => handle_stats(args),
    }
}
