//! Settings actions module
//!
//! This module contains action functions that are called when
//! settings are applied or configured.
//!
//! Most settings have been migrated to the trait-based system
//! in src/settings/definitions/. These remaining modules are kept
//! for functions that are used by multiple places (assist, definitions).

pub mod brightness;
mod mouse;
mod system;

// Re-export public functions that are still used externally
pub use system::launch_cockpit;

