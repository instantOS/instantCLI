// New modular structure
pub mod detection;
pub mod feasibility;
pub mod free_space;
pub mod os_detection;
pub mod parsing;
pub mod resize;
pub mod types;

// Legacy modules
pub mod display;
pub mod verification;

// Re-exports
pub use detection::*;
pub use display::*;
pub use types::*;
pub use verification::*;

/// Minimum required space for Linux installation in bytes (10 GB)
pub const MIN_LINUX_SIZE: u64 = 10 * 1024 * 1024 * 1024;

// Re-export MIN_ESP_SIZE from detection for convenience

pub struct DisksKey;

impl crate::arch::engine::DataKey for DisksKey {
    type Value = Vec<detection::DiskInfo>;
    const KEY: &'static str = "disks";
}
