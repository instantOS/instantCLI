use clap::{Args, Subcommand, ValueEnum};

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
    /// Wallpaper source to fetch from. When omitted, the curated default is
    /// tried first and the remaining sources follow automatically if it fails.
    #[arg(long, value_enum)]
    pub source: Option<WallpaperSource>,
}

/// Source to fetch a random wallpaper from.
///
/// Without an explicit `--source`, `wallhaven` is tried first and the other
/// sources follow when it fails. An explicit source is used strictly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum WallpaperSource {
    /// Curated wallpapers from Wallhaven (tried first)
    Wallhaven,
    /// Random photos from picsum.photos (reliable fallback)
    Picsum,
    /// Recent Bing daily wallpaper (curated, wallpaper-only license)
    Bing,
    /// Random keyworded photos from loremflickr.com
    Loremflickr,
}

impl WallpaperSource {
    /// Canonical lowercase name used in messages and logs.
    pub fn as_str(self) -> &'static str {
        match self {
            WallpaperSource::Wallhaven => "wallhaven",
            WallpaperSource::Picsum => "picsum",
            WallpaperSource::Bing => "bing",
            WallpaperSource::Loremflickr => "loremflickr",
        }
    }
}

impl std::fmt::Display for WallpaperSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
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
