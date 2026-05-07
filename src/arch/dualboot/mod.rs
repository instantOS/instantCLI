// New modular structure
pub mod detection;
pub mod display;
pub mod os_detection;
pub mod parsing;
pub mod resize;
pub mod types;

#[cfg(test)]
pub mod test_utils;

// Re-exports
// From types module (core data structures and utilities)
pub use types::*;

// From detection module
pub use detection::{analyze_all_disks, detect_disks};

// From display module
pub use display::display_disks;

// From resize module
pub use resize::{ResizeStatus, ResizeVerifier};

/// Minimum size for a Linux installation in bytes (30 GB)
pub const MIN_LINUX_SIZE: u64 = 30 * 1024 * 1024 * 1024;

pub struct DisksKey;

impl crate::arch::engine::DataKey for DisksKey {
    type Value = Vec<types::DiskInfo>;
    const KEY: &'static str = "disks";
}
