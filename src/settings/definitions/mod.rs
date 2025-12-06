//! Setting implementations
//!
//! This module contains individual setting implementations using the Setting trait.
//! Settings are registered at compile time via the `inventory` crate.

#[macro_use]
mod command_macros;

pub mod appearance;
pub mod apps;
pub mod brightness;
pub mod desktop;
pub mod keyboard;
pub mod language;
pub mod mouse;
pub mod network;
pub mod packages;
pub mod printers;
pub mod storage;
pub mod swap_escape;
pub mod system;
pub mod toggles;
pub mod users;
pub mod wiremix;

// Note: Settings are auto-registered via inventory::submit! macros in each module.
// They don't need to be re-exported here.
