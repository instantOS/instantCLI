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
// From detection module
pub use detection::{check_all_disks_feasibility, detect_disks};

// From display module
pub use display::display_disks;

// From types module
pub use types::{
    format_size, DetectedOS, DiskInfo, DualBootFeasibility, FilesystemInfo, FreeRegion,
    MIN_ESP_SIZE, OSType, PartitionInfo, PartitionTableType, ResizeInfo,
};

// From verification module
pub use verification::{ResizeStatus, ResizeVerifier};

/// Minimum required space for Linux installation in bytes (10 GB)
pub const MIN_LINUX_SIZE: u64 = 10 * 1024 * 1024 * 1024;

// Re-export MIN_ESP_SIZE from detection for convenience

pub struct DisksKey;

impl crate::arch::engine::DataKey for DisksKey {
    type Value = Vec<types::DiskInfo>;
    const KEY: &'static str = "disks";
}
