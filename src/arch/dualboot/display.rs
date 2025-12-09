//! Display formatting for dual boot detection results
//!
//! Provides pretty-printed output for disk and partition information.
//! Uses simple row-based output similar to `ins arch info`.

use crate::ui::nerd_font::NerdFont;
use colored::Colorize;

use super::{DiskInfo, OSType, PartitionInfo};

/// Display all detected disks with their partitions
pub fn display_disks(disks: &[DiskInfo]) {
    if disks.is_empty() {
        println!(
            "  {} {}",
            NerdFont::Warning.to_string().yellow(),
            "No disks detected.".yellow()
        );
        return;
    }

    for disk in disks {
        display_disk(disk);
        println!();
    }
}

/// Display a single disk with its partitions
pub fn display_disk(disk: &DiskInfo) {
    // Disk header
    println!(
        "  {} {} {} ({})",
        NerdFont::HardDrive.to_string().bright_cyan(),
        disk.device.bold(),
        format!("[{}]", disk.partition_table).dimmed(),
        disk.size_human().bright_white()
    );
    println!("  {}", "─".repeat(60).bright_black());

    if disk.partitions.is_empty() {
        println!(
            "    {} {}",
            NerdFont::Bullet.to_string().dimmed(),
            "No partitions".dimmed()
        );
    } else {
        for partition in &disk.partitions {
            display_partition_row(partition);
        }
    }
}

/// Display a single partition as a row
fn display_partition_row(partition: &PartitionInfo) {
    // Extract partition name from full path (/dev/nvme0n1p2 -> nvme0n1p2)
    let name = partition
        .device
        .strip_prefix("/dev/")
        .unwrap_or(&partition.device);

    // Filesystem type
    let fs_type = partition
        .filesystem
        .as_ref()
        .map(|f| f.fs_type.as_str())
        .unwrap_or("-");

    // Format type/fs string like "ext4 [83]" or just "ext4"
    let type_str = match &partition.partition_type {
        Some(pt) => format!("{} [{}]", fs_type, pt),
        None => fs_type.to_string(),
    };

    // OS detection with icon - EFI partitions get special treatment
    let (os_icon, os_text) = if partition.is_efi {
        (
            NerdFont::Efi.to_string(),
            "EFI System Partition".cyan().to_string(),
        )
    } else {
        match &partition.detected_os {
            Some(os) => {
                let icon = match os.os_type {
                    OSType::Windows => NerdFont::Desktop,
                    OSType::Linux => NerdFont::Terminal,
                    OSType::MacOS => NerdFont::Desktop,
                    OSType::Unknown => NerdFont::Question,
                };
                let text = match os.os_type {
                    OSType::Windows => os.name.blue(),
                    OSType::Linux => os.name.green(),
                    OSType::MacOS => os.name.magenta(),
                    OSType::Unknown => os.name.white(),
                };
                (icon.to_string(), text.to_string())
            }
            None => ("".to_string(), "-".dimmed().to_string()),
        }
    };

    // Resize info - EFI partitions show reuse message in green
    let resize_text = match &partition.resize_info {
        Some(info) if partition.is_efi => {
            // EFI partitions: show reuse message
            let reason = info
                .reason
                .as_ref()
                .cloned()
                .unwrap_or_else(|| "Reuse for dual boot".to_string());
            format!("{} {}", NerdFont::Check.to_string().green(), reason.green())
        }
        Some(info) if info.can_shrink => {
            if let Some(min) = info.min_size_human() {
                format!("{} min: {}", NerdFont::Check.to_string().green(), min)
            } else {
                format!("{} shrinkable", NerdFont::Check.to_string().green())
            }
        }
        Some(info) => {
            let reason = info
                .reason
                .as_ref()
                .cloned()
                .unwrap_or_else(|| "Not shrinkable".to_string());
            format!("{} {}", NerdFont::Cross.to_string().red(), reason.dimmed())
        }
        None => "-".dimmed().to_string(),
    };

    println!(
        "    {} {:>14} {:>10}  {:<12}  {} {}",
        NerdFont::Bullet.to_string().dimmed(),
        name,
        partition.size_human().bright_white(),
        type_str.cyan(),
        os_icon,
        os_text
    );

    // Show resize info on separate line if present and interesting
    if let Some(info) = &partition.resize_info {
        if info.can_shrink || info.reason.is_some() {
            println!(
                "      {} {}",
                NerdFont::ArrowSubItem.to_string().dimmed(),
                resize_text
            );
        }

        // Show prerequisites if any
        if !info.prerequisites.is_empty() {
            for prereq in &info.prerequisites {
                println!(
                    "        {} {}",
                    NerdFont::ArrowPointer.to_string().dimmed(),
                    prereq.yellow()
                );
            }
        }
    }
}

/// Display detailed information about a partition's resize constraints
pub fn display_resize_details(partition: &PartitionInfo) {
    println!(
        "\n{} {}",
        "Resize details for:".bold(),
        partition.device.cyan()
    );

    if let Some(info) = &partition.resize_info {
        if info.can_shrink {
            println!(
                "  {} This partition can be shrunk",
                NerdFont::Check.to_string().green().bold()
            );
            if let Some(min) = info.min_size_human() {
                println!(
                    "  {} Minimum size: {}",
                    NerdFont::ArrowPointer.to_string().dimmed(),
                    min.yellow()
                );
            }
        } else {
            println!(
                "  {} This partition cannot be shrunk",
                NerdFont::Cross.to_string().red().bold()
            );
        }

        if let Some(reason) = &info.reason {
            println!(
                "  {} Reason: {}",
                NerdFont::ArrowPointer.to_string().dimmed(),
                reason
            );
        }

        if !info.prerequisites.is_empty() {
            println!("\n  {}:", "Prerequisites".bold());
            for prereq in &info.prerequisites {
                println!("    {} {}", NerdFont::Bullet.to_string().dimmed(), prereq);
            }
        }
    } else {
        println!(
            "  {} No resize information available",
            NerdFont::Question.to_string().yellow()
        );
    }
}
