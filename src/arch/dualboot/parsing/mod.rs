//! Parsing utilities for lsblk and sfdisk output

mod lsblk;
mod sfdisk;

// Re-export public functions from submodules
pub use lsblk::parse_partition;
pub use sfdisk::get_free_regions;
