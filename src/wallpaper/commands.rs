use anyhow::Result;
use colored::*;

use crate::common::compositor::CompositorType;
use crate::wallpaper::cli::{SetArgs, WallpaperCommands};
use crate::wallpaper::config::WallpaperConfig;

use crate::wallpaper::{sway, x11};

pub async fn handle_wallpaper_command(command: WallpaperCommands, _debug: bool) -> Result<()> {
    match command {
        WallpaperCommands::Set(args) => handle_set(args).await,
        WallpaperCommands::Apply => apply_configured_wallpaper().await,
    }
}

async fn handle_set(args: SetArgs) -> Result<()> {
    let mut config = WallpaperConfig::load()?;
    config.set_wallpaper(args.path.clone())?;
    println!("Wallpaper configured to: {}", args.path.green());

    // Apply the wallpaper after setting it
    apply_configured_wallpaper().await
}

pub async fn apply_configured_wallpaper() -> Result<()> {
    let config = WallpaperConfig::load()?;
    let path = match config.path {
        Some(p) => p,
        None => {
            anyhow::bail!("No wallpaper configured. Use 'ins wallpaper set <path>' first.");
        }
    };

    let compositor = CompositorType::detect();
    println!("Applying wallpaper to {}...", compositor.name().cyan());

    match compositor {
        CompositorType::Sway => {
            sway::apply_wallpaper(&path)?;
            println!("{}", "Wallpaper applied successfully".green());
        }
        CompositorType::I3 | CompositorType::InstantWM => {
            x11::apply_wallpaper(&path)?;
            println!("{}", "Wallpaper applied successfully".green());
        }
        _ => {
            println!(
                "{}",
                format!(
                    "Wallpaper setting not yet supported for {}",
                    compositor.name()
                )
                .yellow()
            );
        }
    }

    Ok(())
}
