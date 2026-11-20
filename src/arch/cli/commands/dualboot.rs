use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt};

use anyhow::{Context, Result, ensure};
use colored::Colorize;
use serde_json::{Value, json};

use crate::arch::disks::{ensure_not_running_disk, probe_exclusive_access};
use crate::arch::dualboot::{
    DiskAnalysis, MIN_DUAL_BOOT_RESIZE_BYTES, MIN_LINUX_SIZE, Shrinkability, analyze_all_disks,
    display_disks,
};
use crate::ui::nerd_font::NerdFont;

/// Handle dual boot info display
pub(super) async fn handle_dualboot_info() -> Result<()> {
    let analyses = analyze_all_disks().context("Could not inspect disks for dual boot")?;
    if crate::ui::get_output_format() == crate::ui::OutputFormat::Json {
        let disks = analyses
            .iter()
            .map(|analysis| disk_report(analysis, inspect_target(&analysis.disk.device)))
            .collect::<Vec<_>>();
        println!("{}", json!({ "disks": disks }));
        return Ok(());
    }

    println!();
    println!(
        "  {} {}",
        NerdFont::HardDrive.to_string().bright_cyan(),
        "Dual Boot Detection".bright_white().bold()
    );
    println!("  {}", "─".repeat(50).bright_black());
    println!();

    let mut any_feasible = false;
    let mut any_unknown = false;

    // Show feasibility summary first
    println!("  {}", "Feasibility Summary:".bold());
    for analysis in &analyses {
        let disk = &analysis.disk.device;
        let feasibility = &analysis.feasibility;
        if feasibility.feasible {
            any_feasible = true;
            // Show different message depending on whether feasibility
            // comes from resizable partitions or unpartitioned space
            if feasibility.feasible_partitions.is_empty() {
                // Feasible due to unpartitioned space
                println!(
                    "    {} {} - {}",
                    NerdFont::Check.to_string().green().bold(),
                    disk.bright_white(),
                    "FEASIBLE".green().bold(),
                );
                if let Some(reason) = &feasibility.reason {
                    println!(
                        "      {} {}",
                        NerdFont::ArrowPointer.to_string().dimmed(),
                        reason.green()
                    );
                }
            } else {
                // Feasible due to resizable partitions
                println!(
                    "    {} {} - {} {}",
                    NerdFont::Check.to_string().green().bold(),
                    disk.bright_white(),
                    "FEASIBLE".green().bold(),
                    format!(
                        "({} partition(s) can be resized)",
                        feasibility.feasible_partitions.len()
                    )
                    .dimmed()
                );
                for part in &feasibility.feasible_partitions {
                    println!(
                        "      {} {}",
                        NerdFont::ArrowPointer.to_string().dimmed(),
                        part.cyan()
                    );
                }
            }
        } else {
            let unknown = analysis.disk.max_contiguous_free_space_bytes.is_err();
            any_unknown |= unknown;
            let status = if unknown {
                "UNKNOWN".yellow().bold()
            } else {
                "NOT FEASIBLE".red().bold()
            };
            println!(
                "    {} {} - {}",
                if unknown {
                    NerdFont::Question.to_string().yellow().bold()
                } else {
                    NerdFont::Cross.to_string().red().bold()
                },
                disk.bright_white(),
                status,
            );
            if let Some(reason) = &feasibility.reason {
                println!(
                    "      {} {}",
                    NerdFont::ArrowPointer.to_string().dimmed(),
                    reason.dimmed()
                );
            }
        }
    }

    println!();

    if any_feasible {
        println!(
            "  {} {}",
            NerdFont::Check.to_string().green().bold(),
            "Dual boot is POSSIBLE on this system".green().bold()
        );
        println!(
            "  {} The installer will offer dual boot options",
            NerdFont::ArrowPointer.to_string().dimmed()
        );
    } else if any_unknown {
        println!(
            "  {} {}",
            NerdFont::Question.to_string().yellow().bold(),
            "Dual boot feasibility is unknown for one or more disks"
                .yellow()
                .bold()
        );
    } else {
        println!(
            "  {} {}",
            NerdFont::Cross.to_string().red().bold(),
            "Dual boot is NOT POSSIBLE on this system".red().bold()
        );
        println!(
            "  {} The installer will NOT offer dual boot options",
            NerdFont::ArrowPointer.to_string().dimmed()
        );
    }

    println!();
    println!("  {}", "Detailed Disk Information:".bold());
    println!("  {}", "─".repeat(50).bright_black());
    println!();

    // Show detailed disk information
    if analyses.is_empty() {
        println!(
            "  {} No disks detected. Are you running as root?",
            NerdFont::Warning.to_string().yellow()
        );
    } else {
        let disks: Vec<_> = analyses.iter().map(|a| &a.disk).collect();
        display_disks(&disks);
    }

    Ok(())
}

fn inspect_target(path: &str) -> Result<()> {
    ensure_not_running_disk(path)?;
    let metadata = fs::metadata(path).with_context(|| format!("Could not inspect {path}"))?;
    ensure!(
        metadata.file_type().is_block_device(),
        "{path} is not a block device"
    );

    let device = metadata.rdev();
    let major = nix::libc::major(device);
    let minor = nix::libc::minor(device);
    let read_only = fs::read_to_string(format!("/sys/dev/block/{major}:{minor}/ro"))
        .with_context(|| format!("Could not check whether {path} is read-only"))?;
    ensure!(read_only.trim() == "0", "{path} is read-only");
    probe_exclusive_access(path).with_context(|| format!("Could not access {path} exclusively"))
}

fn disk_report(analysis: &DiskAnalysis, target: Result<()>) -> Value {
    let disk = &analysis.disk;
    let free_space = disk.max_contiguous_free_space_bytes.as_ref();
    let existing_free_space = disk.has_sufficient_free_space();
    let mut resize_options = Vec::new();
    if free_space.is_ok() {
        for partition in &disk.partitions {
            if !analysis
                .feasibility
                .feasible_partitions
                .contains(&partition.device)
            {
                continue;
            }
            let Some(info) = &partition.resize_info else {
                continue;
            };
            if let Shrinkability::Shrinkable { min_size_bytes } = info.shrinkability {
                let max_size = partition.size_bytes.saturating_sub(min_size_bytes);
                if max_size >= MIN_DUAL_BOOT_RESIZE_BYTES {
                    resize_options.push(json!({
                        "partition": partition.device,
                        "minimum_linux_space_bytes": MIN_DUAL_BOOT_RESIZE_BYTES,
                        "maximum_linux_space_bytes": max_size,
                        "minimum_remaining_partition_bytes": min_size_bytes,
                        "prerequisites": info.prerequisites,
                    }));
                }
            }
        }
    }

    let auto_partitioning_available = target.is_ok();
    let target_error = target.err().map(|error| format!("{error:#}"));
    json!({
        "device": disk.device,
        "size_bytes": disk.size_bytes,
        "partition_table": disk.partition_table,
        "partitions": disk.partitions,
        "auto_partitioning_available": auto_partitioning_available,
        "target_error": target_error,
        "largest_free_region_bytes": free_space.ok(),
        "free_space_error": free_space.err(),
        "dual_boot": {
            "available": auto_partitioning_available && (existing_free_space || !resize_options.is_empty()),
            "can_use_existing_free_space": existing_free_space,
            "minimum_linux_space_bytes": MIN_LINUX_SIZE,
            "reusable_efi_partition": disk.find_reusable_esp().map(|partition| &partition.device),
            "resize_options": resize_options,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::{MIN_DUAL_BOOT_RESIZE_BYTES, MIN_LINUX_SIZE, disk_report};
    use crate::arch::dualboot::{
        DiskAnalysis, DiskInfo, FilesystemInfo, PartitionInfo, PartitionTableType, ResizeInfo,
        Shrinkability,
    };
    use crate::common::blockdev::Filesystem;

    #[test]
    fn disk_report_distinguishes_resize_options_from_unavailable_targets() {
        let disk = DiskInfo {
            device: "/dev/test".into(),
            size_bytes: 120 * 1024 * 1024 * 1024,
            partition_table: PartitionTableType::GPT,
            partitions: vec![PartitionInfo {
                device: "/dev/test1".into(),
                size_bytes: 2 * MIN_DUAL_BOOT_RESIZE_BYTES,
                filesystem: Some(FilesystemInfo {
                    fs_type: Filesystem::parse("ext4"),
                    uuid: None,
                    label: None,
                }),
                detected_os: None,
                resize_info: Some(ResizeInfo {
                    shrinkability: Shrinkability::Shrinkable {
                        min_size_bytes: MIN_DUAL_BOOT_RESIZE_BYTES,
                    },
                    prerequisites: vec!["unmount before resizing".into()],
                }),
                mount_point: None,
                is_efi: false,
                partition_type: None,
            }],
            max_contiguous_free_space_bytes: Ok(MIN_LINUX_SIZE),
        };
        let mut analysis = DiskAnalysis {
            feasibility: disk.check_disk_dualboot_feasibility(),
            disk,
        };

        let report = disk_report(&analysis, Ok(()));
        assert_eq!(report["auto_partitioning_available"], true);
        assert_eq!(report["dual_boot"]["available"], true);
        assert_eq!(report["dual_boot"]["can_use_existing_free_space"], true);
        let option = &report["dual_boot"]["resize_options"][0];
        assert_eq!(option["partition"], "/dev/test1");
        assert_eq!(
            option["maximum_linux_space_bytes"],
            MIN_DUAL_BOOT_RESIZE_BYTES
        );

        let blocked = disk_report(&analysis, Err(anyhow::anyhow!("device is mounted")));
        assert_eq!(blocked["auto_partitioning_available"], false);
        assert_eq!(blocked["dual_boot"]["available"], false);
        assert_eq!(blocked["target_error"], "device is mounted");

        analysis.disk.max_contiguous_free_space_bytes = Err("Permission denied".into());
        analysis.feasibility = analysis.disk.check_disk_dualboot_feasibility();
        assert!(!analysis.feasibility.feasible);
        assert!(analysis.feasibility.feasible_partitions.is_empty());
        assert!(
            analysis
                .feasibility
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains("Permission denied"))
        );
        let unknown_layout = disk_report(&analysis, Ok(()));
        assert!(unknown_layout["largest_free_region_bytes"].is_null());
        assert_eq!(unknown_layout["dual_boot"]["available"], false);
        assert_eq!(unknown_layout["free_space_error"], "Permission denied");
    }
}
