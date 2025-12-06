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
pub use system::launch_cockpit;
