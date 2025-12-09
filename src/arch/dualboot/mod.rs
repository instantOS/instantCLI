pub mod detection;
pub mod display;
pub mod verification;

pub use detection::*;
pub use display::*;
pub use verification::*;

/// Minimum required space for Linux installation in bytes (10 GB)
pub const MIN_LINUX_SIZE: u64 = 10 * 1024 * 1024 * 1024;

// Re-export MIN_ESP_SIZE from detection for convenience
pub use detection::MIN_ESP_SIZE;

pub struct DisksKey;

impl crate::arch::engine::DataKey for DisksKey {
    type Value = Vec<detection::DiskInfo>;
    const KEY: &'static str = "disks";
}
