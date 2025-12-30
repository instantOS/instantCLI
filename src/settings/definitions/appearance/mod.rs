//! Appearance settings
//!
//! Theming, animations, wallpaper, cursor, and dark mode settings.

mod animations;
mod common;
mod cursor;
mod dark_mode;
mod gtk;
mod qt;
mod wallpaper;

// Re-export all setting structs
pub use animations::Animations;
pub use cursor::CursorTheme;
pub use dark_mode::DarkMode;
pub use gtk::{GtkIconTheme, GtkMenuIcons, GtkTheme, ResetGtk};
pub use qt::ResetQt;
pub use wallpaper::{
    ApplyColoredWallpaper, RandomWallpaper, SetWallpaper, WallpaperBgColor, WallpaperFgColor,
    WallpaperLogo,
};
