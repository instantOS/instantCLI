//! Display formatting for dual boot detection results
//!
//! Provides pretty-printed output for disk and partition information.
//! Uses simple row-based output similar to `ins arch info`.

use crate::arch::dualboot::types::{DiskInfo, OSType, PartitionInfo};
use crate::ui::nerd_font::NerdFont;
use colored::Colorize;

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
        display_disk(disk);
        println!();
    }
}

/// Display a disk with its partitions
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
pub fn display_partition_row(partition: &PartitionInfo) {
    let name = partition
        .device
        .strip_prefix("/dev/")
        .unwrap_or(&partition.device);

    let fs_type = partition
        .filesystem
        .as_ref()
        .map(|f| f.fs_type.as_str())
        .unwrap_or("-");

    let type_str = match &partition.partition_type {
        Some(pt) => format!("{} [{}]", fs_type, pt),
        None => fs_type.to_string(),
    };

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

    let resize_text = match &partition.resize_info {
        Some(info) if partition.is_efi => {
            let reason = info
                .reason
                .as_ref()
                .cloned()
                .unwrap_or_else(|| "Reuse for dual boot".to_string());
            format!(
                "{} {}",
                NerdFont::Check.to_string().green(),
                reason.green()
            )
        }
        Some(info) if info.can_shrink => {
            if let Some(min) = info.min_size_human() {
                format!(
                    "{} min: {}",
                    NerdFont::Check.to_string().green(),
                    min
                )
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
            format!(
                "{} {}",
                NerdFont::Cross.to_string().red(),
                reason.dimmed()
            )
        }
        None => "-".dimmed().to_string(),
    };

    println!(
        "    {} {:<14} {:>10}  {:<12}  {} {}",
        NerdFont::Bullet.to_string().dimmed(),
        name,
        partition.size_human().bright_white(),
        type_str.cyan(),
        os_icon,
        os_text
    );

    if let Some(info) = &partition.resize_info {
        if info.can_shrink || info.reason.is_some() {
            println!(
                "      {} {}",
                NerdFont::ArrowSubItem.to_string().dimmed(),
                resize_text
            );
        }

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
