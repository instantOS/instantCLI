//! Setting implementations
//!
//! This module contains individual setting implementations using the Setting trait.
//! Settings are registered at compile time via the `inventory` crate.

#[macro_use]
mod command_macros;

mod appearance;
mod apps;
mod brightness;
mod desktop;
mod keyboard;
mod language;
mod mouse;
mod network;
mod packages;
mod printers;
mod storage;
mod swap_escape;
mod system;
mod toggles;
mod users;
mod wiremix;

// Note: Settings are auto-registered via inventory::submit! macros in each module.
// They don't need to be re-exported here.
