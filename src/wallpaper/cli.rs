use clap::{Args, Subcommand};

#[derive(Subcommand, Debug, Clone)]
pub enum WallpaperCommands {
    /// Set the wallpaper
    Set(SetArgs),
    /// Apply the currently configured wallpaper
    Apply,
    /// Fetch and set a random wallpaper
    Random(RandomArgs),
}

#[derive(Args, Debug, Clone)]
pub struct SetArgs {
    /// Path to the wallpaper image
    pub path: String,
}

#[derive(Args, Debug, Clone)]
pub struct RandomArgs {
    /// Do not apply the instantOS logo overlay
    #[arg(long)]
    pub no_logo: bool,
}
