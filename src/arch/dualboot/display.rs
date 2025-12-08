//! Display formatting for dual boot detection results
//!
//! Provides pretty-printed output for disk and partition information.

use colored::Colorize;

use super::{DiskInfo, OSType, PartitionInfo};

/// Display all detected disks with their partitions
pub fn display_disks(disks: &[DiskInfo]) {
    if disks.is_empty() {
        println!("{}", "No disks detected.".yellow());
        return;
    }

    for disk in disks {
        display_disk(disk);
        println!();
    }
}

/// Display a single disk with its partitions
pub fn display_disk(disk: &DiskInfo) {
    // Header
    let header = format!(
        " Disk: {} ({}) - {} ",
        disk.device.bold(),
        disk.size_human,
        disk.partition_table
    );

    let width = 78;
    let header_padded = format!("{:^width$}", header, width = width);

    println!("╭{}╮", "─".repeat(width));
    println!("│{}│", header_padded.bold());
    println!("├{}┤", "─".repeat(width));

    // Column headers
    println!(
        "│ {:<16} {:<8} {:<10} {:<20} {:<18} │",
        "Partition".bold(),
        "Type".bold(),
        "Size".bold(),
        "OS".bold(),
        "Shrinkable".bold()
    );
    println!("│{}│", "─".repeat(width));

    if disk.partitions.is_empty() {
        println!("│{:^width$}│", "No partitions", width = width);
    } else {
        for partition in &disk.partitions {
            display_partition_row(partition);
        }
    }

    println!("╰{}╯", "─".repeat(width));
}

/// Display a single partition as a table row
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

    // Size
    let size = &partition.size_human;

    // OS detection
    let os_info = match &partition.detected_os {
        Some(os) => {
            let name = if os.name.len() > 18 {
                format!("{}…", &os.name[..17])
            } else {
                os.name.clone()
            };
            match os.os_type {
                OSType::Windows => name.blue().to_string(),
                OSType::Linux => name.green().to_string(),
                OSType::MacOS => name.magenta().to_string(),
                OSType::Unknown => name.white().to_string(),
            }
        }
        None => "-".dimmed().to_string(),
    };

    // Resize info
    let resize_info = match &partition.resize_info {
        Some(info) if info.can_shrink => {
            if let Some(min) = &info.min_size_human {
                format!("✓ Min: {}", min).green().to_string()
            } else {
                "✓ Yes".green().to_string()
            }
        }
        Some(info) => {
            let reason = info
                .reason
                .as_ref()
                .map(|r| {
                    if r.len() > 12 {
                        format!("{}…", &r[..11])
                    } else {
                        r.clone()
                    }
                })
                .unwrap_or_else(|| "No".to_string());
            format!("✗ {}", reason).red().to_string()
        }
        None => "-".dimmed().to_string(),
    };

    // Calculate visible widths (accounting for ANSI codes)
    // We need to use fixed widths and truncate appropriately
    println!(
        "│ {:<16} {:<8} {:<10} {:<20} {:<18} │",
        name, fs_type, size, os_info, resize_info
    );
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
            println!("  {} This partition can be shrunk", "✓".green().bold());
            if let Some(min) = &info.min_size_human {
                println!("  {} Minimum size: {}", "→".dimmed(), min.yellow());
            }
        } else {
            println!("  {} This partition cannot be shrunk", "✗".red().bold());
        }

        if let Some(reason) = &info.reason {
            println!("  {} Reason: {}", "→".dimmed(), reason);
        }

        if !info.prerequisites.is_empty() {
            println!("\n  {}:", "Prerequisites".bold());
            for prereq in &info.prerequisites {
                println!("    {} {}", "•".dimmed(), prereq);
            }
        }
    } else {
        println!("  {} No resize information available", "?".yellow());
    }
}
