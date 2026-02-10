use anyhow::Result;

use super::audio;
use super::cli::VideoCommands;
use super::menu;
use super::pipeline::{check, convert, setup, transcribe};
use super::render;
use super::slides;

pub async fn handle_video_command(command: VideoCommands, _debug: bool) -> Result<()> {
    match command {
        VideoCommands::Convert(args) => convert::handle_convert(args).await,
        VideoCommands::Append(args) => convert::handle_append(args).await,
        VideoCommands::Transcribe(args) => transcribe::handle_transcribe(args),
        VideoCommands::Render(args) => render::handle_render(args).await.map(|_| ()),
        VideoCommands::Slide(args) => slides::cli::handle_slide(args),
        VideoCommands::Check(args) => check::handle_check(args).await,
        VideoCommands::Preprocess(args) => audio::handle_preprocess(args).await,
        VideoCommands::Setup(args) => setup::handle_setup(args).await,
        VideoCommands::Menu => menu::video_menu(_debug).await,
    }
}
