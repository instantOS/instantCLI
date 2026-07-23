//! Notification center for instantOS
//!
//! Provides a notification history browser and D-Bus capture daemon,
//! replacing the legacy instantNOTIFY bash scripts with a unified
//! Rust implementation that works on both X11 (dunst) and Wayland (mako).

pub mod capture;
pub mod commands;
pub mod db;
pub mod handlers;
pub mod items;
pub mod menu;
pub mod options;
mod service;

pub use commands::{NotifyCommands, handle_notify_command};
