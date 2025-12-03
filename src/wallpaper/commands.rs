use anyhow::Result;
use colored::*;

use crate::common::compositor::CompositorType;
use crate::wallpaper::cli::{WallpaperCommands, SetArgs};
use crate::wallpaper::config::WallpaperConfig;

pub async fn handle_wallpaper_command(command: WallpaperCommands, _debug: bool) -> Result<()> {
    let compositor = CompositorType::detect();
    println!("Detected compositor: {}", compositor.name().cyan());

    match command {
        WallpaperCommands::Set(args) => handle_set(args).await,
    }
}

async fn handle_set(args: SetArgs) -> Result<()> {
    let mut config = WallpaperConfig::load()?;
    config.set_wallpaper(args.path.clone())?;
    println!("Wallpaper configured to: {}", args.path.green());
    Ok(())
}
