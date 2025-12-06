//! Setting implementations
//!
//! This module contains individual setting implementations using the Setting trait.
//! Settings are registered at compile time via the `inventory` crate.

mod brightness;
mod swap_escape;
mod wiremix;

// Note: Settings are auto-registered via inventory::submit! macros in each module.
// They don't need to be re-exported here.
