// New modular structure
pub mod types;
pub mod detection;
pub mod free_space;
pub mod feasibility;
pub mod os_detection;
pub mod resize;
pub mod parsing;

// Legacy modules
pub mod display;
pub mod verification;

// Re-exports
pub use types::*;
pub use detection::*;
pub use free_space::*;
pub use feasibility::*;
pub use os_detection::*;
pub use resize::*;
pub use display::*;
pub use verification::*;

/// Minimum required space for Linux installation in bytes (10 GB)
pub const MIN_LINUX_SIZE: u64 = 10 * 1024 * 1024 * 1024;

// Re-export MIN_ESP_SIZE from detection for convenience

pub struct DisksKey;

impl crate::arch::engine::DataKey for DisksKey {
    type Value = Vec<detection::DiskInfo>;
    const KEY: &'static str = "disks";
}
