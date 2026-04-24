//! Display formatting for dual boot detection results
//!
//! Provides pretty-printed output for disk and partition information.
//! Uses simple row-based output similar to `ins arch info`.

use crate::ui::nerd_font::NerdFont;
use colored::Colorize;

use super::DiskInfo;

/// Display all detected disks with their partitions
pub fn display_disks(disks: &[&DiskInfo]) {
    if disks.is_empty() {
        println!(
            "  {} {}",
            NerdFont::Warning.to_string().yellow(),
            "No disks detected.".yellow()
        );
        return;
    }

    for disk in disks {
        disk.display_disk();
        println!();
    }
}
