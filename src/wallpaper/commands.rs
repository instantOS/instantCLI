use anyhow::{Context, Result};
use colored::*;
use std::path::PathBuf;

use crate::common::compositor::CompositorType;
use crate::settings::store::{
    SettingsStore, WALLPAPER_BG_COLOR_KEY, WALLPAPER_FG_COLOR_KEY, WALLPAPER_LOGO_KEY,
    WALLPAPER_PATH_KEY,
};
use crate::wallpaper::cli::{SetArgs, WallpaperCommands};

use crate::wallpaper::{sway, x11};

pub async fn handle_wallpaper_command(command: WallpaperCommands, _debug: bool) -> Result<()> {
    match command {
        WallpaperCommands::Set(args) => handle_set(args).await,
        WallpaperCommands::Apply => apply_configured_wallpaper().await,
        WallpaperCommands::Random(args) => handle_random(args).await,
        WallpaperCommands::Colored(args) => handle_colored(args).await,
    }
}

async fn handle_random(args: crate::wallpaper::cli::RandomArgs) -> Result<()> {
    // If --no-logo flag is explicitly passed, use it; otherwise check settings
    let no_logo = if args.no_logo {
        true
    } else {
        let store = SettingsStore::load().context("loading settings")?;
        !store.bool(WALLPAPER_LOGO_KEY)
    };

    let path =
        crate::wallpaper::random::run(crate::wallpaper::random::RandomOptions { no_logo }).await?;

    println!(
        "Generated wallpaper at: {}",
        path.display().to_string().green()
    );

    // Set and apply
    handle_set(SetArgs {
        path: path.to_string_lossy().to_string(),
    })
    .await
}

async fn handle_set(args: SetArgs) -> Result<()> {
    let mut store = SettingsStore::load().context("loading settings")?;

    // Resolve absolute path
    let path_buf = PathBuf::from(&args.path);
    let abs_path = if path_buf.is_absolute() {
        args.path.clone()
    } else {
        std::env::current_dir()
            .context("getting current directory")?
            .join(&args.path)
            .to_string_lossy()
            .to_string()
    };

    store.set_optional_string(WALLPAPER_PATH_KEY, Some(abs_path.clone()));
    store.save().context("saving settings")?;
    println!("Wallpaper configured to: {}", abs_path.green());

    // Apply the wallpaper after setting it
    apply_configured_wallpaper().await
}

pub async fn apply_configured_wallpaper() -> Result<()> {
    let store = SettingsStore::load().context("loading settings")?;
    let path = match store.optional_string(WALLPAPER_PATH_KEY) {
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
        CompositorType::I3 | CompositorType::Dwm | CompositorType::InstantWM => {
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

async fn handle_colored(args: crate::wallpaper::cli::ColoredArgs) -> Result<()> {
    let mut store = SettingsStore::load().context("loading settings")?;

    // Use CLI args or fall back to saved settings, or use defaults
    let bg_color = args
        .bg
        .or_else(|| store.optional_string(WALLPAPER_BG_COLOR_KEY))
        .unwrap_or_else(|| "#1a1a2e".to_string());
    let fg_color = args
        .fg
        .or_else(|| store.optional_string(WALLPAPER_FG_COLOR_KEY))
        .unwrap_or_else(|| "#eaeaea".to_string());

    // Save colors for future use
    store.set_optional_string(WALLPAPER_BG_COLOR_KEY, Some(bg_color.clone()));
    store.set_optional_string(WALLPAPER_FG_COLOR_KEY, Some(fg_color.clone()));
    store.save().context("saving settings")?;

    let path = crate::wallpaper::colored::run(crate::wallpaper::colored::ColoredOptions {
        bg_color,
        fg_color,
    })
    .await?;

    println!(
        "Generated wallpaper at: {}",
        path.display().to_string().green()
    );

    // Set and apply
    handle_set(SetArgs {
        path: path.to_string_lossy().to_string(),
    })
    .await
}
