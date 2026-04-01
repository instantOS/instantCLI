//! Keyboard settings module
//!
//! Contains keyboard layout settings for desktop sessions, TTY, and login screens.

mod common;
mod layout;
mod login;
mod tty;

pub use layout::KeyboardLayout;
pub use login::LoginScreenLayout;
pub use tty::TtyKeymap;

// Re-export functions used by preview/keyboard.rs
pub use common::{
    current_gnome_layouts, current_vconsole_keymap, current_x11_layout, current_x11_layouts,
};
