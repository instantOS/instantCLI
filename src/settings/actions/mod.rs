//! Settings actions module
//!
//! This module contains all the action functions that are called when
//! settings are applied or configured.

mod bluetooth;
pub mod brightness;
mod desktop;
mod keyboard;
mod mouse;
mod storage;
mod system;

// Re-export all public functions
pub use bluetooth::apply_bluetooth_service;
pub use brightness::{configure_brightness, restore_brightness};
pub use desktop::{apply_clipboard_manager, pick_and_set_wallpaper};
pub use keyboard::{configure_keyboard_layout, restore_keyboard_layout};
pub use mouse::{
    apply_natural_scroll, apply_swap_buttons, configure_mouse_sensitivity,
    restore_mouse_sensitivity,
};
pub use storage::apply_udiskie_automount;
pub use system::{apply_pacman_autoclean, configure_timezone, launch_cockpit};
