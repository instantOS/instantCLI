//! Settings actions module
//!
//! This module contains action functions that are called when
//! settings are applied or configured.
//!
//! Note: Most settings have been migrated to the trait-based system
//! in src/settings/definitions/. These remaining exports are kept
//! for backward compatibility or external usage.

mod bluetooth;
pub mod brightness;
mod desktop;
mod keyboard;
mod mouse;
mod storage;
mod system;

// Re-export public functions that are still used externally
pub use bluetooth::apply_bluetooth_service;
pub use brightness::configure_brightness;
pub use desktop::{
    apply_clipboard_manager, apply_colored_wallpaper, pick_and_set_wallpaper,
    pick_wallpaper_bg_color, pick_wallpaper_fg_color, set_random_wallpaper,
};
pub use keyboard::{apply_swap_escape, configure_keyboard_layout};
pub use mouse::{apply_natural_scroll, apply_swap_buttons, configure_mouse_sensitivity};
pub use storage::apply_udiskie_automount;
pub use system::{apply_pacman_autoclean, configure_timezone, launch_cockpit};
