use anyhow::Result;

use super::cli::VideoCommands;
use super::convert::handle_convert;

pub fn handle_video_command(command: VideoCommands, _debug: bool) -> Result<()> {
    match command {
        VideoCommands::Convert(args) => handle_convert(args),
    }
}
