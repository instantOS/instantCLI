use clap::{Args, Subcommand};

#[derive(Subcommand, Debug, Clone)]
pub enum WallpaperCommands {
    /// Set the wallpaper
    Set(SetArgs),
    /// Apply the currently configured wallpaper
    Apply,
    /// Fetch and set a random wallpaper
    Random(RandomArgs),
    /// Generate a colored wallpaper with the instantOS logo
    Colored(ColoredArgs),
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

#[derive(Args, Debug, Clone)]
pub struct ColoredArgs {
    /// Background color in hex format (e.g., #1a1a2e). Uses saved setting if omitted.
    #[arg(long, short = 'b')]
    pub bg: Option<String>,
    /// Foreground/logo color in hex format (e.g., #ffffff). Uses saved setting if omitted.
    #[arg(long, short = 'f')]
    pub fg: Option<String>,
}
