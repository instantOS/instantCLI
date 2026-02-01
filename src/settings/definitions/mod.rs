//! Setting implementations
//!
//! This module contains individual setting implementations using the Setting trait.
//! Settings are organized via the category tree in category_tree.rs.

#[macro_use]
mod command_macros;

pub mod appearance;
pub mod appimages;
pub mod apps;
pub mod brightness;
pub mod desktop;
pub mod display;
pub mod flatpak;
pub mod installed_flatpaks;
pub mod installed_packages;
pub mod installed_snaps;
pub mod keyboard;
pub mod language;
pub mod mouse;
pub mod network;
pub mod packages;
pub mod printers;
pub mod snap;
pub mod storage;
pub mod swap_escape;
pub mod system;
pub mod toggles;
pub mod users;
pub mod wiremix;

// Note: Settings are organized in category_tree.rs.
// They don't need to be re-exported here.
