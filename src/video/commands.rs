use anyhow::Result;

use super::cli::VideoCommands;
use super::convert::handle_convert;
use super::transcribe::handle_transcribe;

pub fn handle_video_command(command: VideoCommands, _debug: bool) -> Result<()> {
    match command {
        VideoCommands::Convert(args) => handle_convert(args),
        VideoCommands::Transcribe(args) => handle_transcribe(args),
    }
}
