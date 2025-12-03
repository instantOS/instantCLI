use clap::{Args, Subcommand};

#[derive(Subcommand, Debug, Clone)]
pub enum WallpaperCommands {
    /// Set the wallpaper
    Set(SetArgs),
    /// Apply the currently configured wallpaper
    Apply,
}

#[derive(Args, Debug, Clone)]
pub struct SetArgs {
    /// Path to the wallpaper image
    pub path: String,
}
