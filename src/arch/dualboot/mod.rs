pub mod detection;
pub mod display;

pub use detection::*;
pub use display::*;

/// Minimum required space for Linux installation in bytes (10 GB)
pub const MIN_LINUX_SIZE: u64 = 10 * 1024 * 1024 * 1024;
