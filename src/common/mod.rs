pub mod audio;
pub mod blockdev;
pub mod commands;
pub mod compositor;
pub mod config;
pub mod deps;
pub mod display;
pub mod display_server;
pub mod distro;
pub mod git;
pub mod instantwm;
pub mod instantwmctl;
pub mod network;
pub mod package;
pub mod pacman;
pub mod paths;
pub mod progress;
pub mod requirements;
pub mod shell;
pub mod systemd;
pub mod terminal;
pub mod tilde_path;

// Re-export commonly used types
pub use tilde_path::{TildePath, home_dir};
