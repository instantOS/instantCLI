use super::mount;
use super::probe::{get_current_partitions, get_partition_size_bytes};
use super::util::{align_down, parse_partition_number};
use crate::arch::dualboot::parsing::{PartitionLayout, get_free_regions, get_partition_layout};
use crate::arch::dualboot::types::{FreeRegion, MIN_ESP_SIZE};
use crate::arch::dualboot::{DisksKey, PartitionTableType};
use crate::arch::engine::{
    DualBootPartitionPaths, DualBootPartitions, EspNeedsFormat, InstallContext, QuestionId,
};
use crate::arch::execution::CommandExecutor;
use anyhow::{Context, Result};
use std::process::Command;

struct ResizePlan {
    preferred_region: FreeRegion,
}

pub fn prepare_dualboot_disk(
    context: &InstallContext,
    executor: &CommandExecutor,
    disk_path: &str,
    mut swap_size_gb: u64,
) -> Result<()> {
    println!("Preparing dual boot installation...");

    let detected = crate::arch::dualboot::detect_disks()
        .context("Disk detection data not available and re-detection failed")?;
    context.set::<DisksKey>(detected.clone());
    let mut disks = detected;

    let mut disk_info = disks
        .iter()
        .find(|d| d.device == disk_path)
        .context("Selected disk not found in detection data")?;

    let mut resized_partition: Option<String> = None;
    let mut resize_plan: Option<ResizePlan> = None;
    let resize_choice = context
        .get_answer(&QuestionId::DualBootInstructions)
        .map(|choice| choice.as_str())
        .unwrap_or("manual");
    let auto_resize_selected = resize_choice == "auto";

    if let Some(partition_path) = context.get_answer(&QuestionId::DualBootPartition)
        && partition_path != "__free_space__"
    {
        resized_partition = Some(partition_path.to_string());

        if auto_resize_selected {
            let size_str = context
                .get_answer(&QuestionId::DualBootSize)
                .context("No size selected for dual boot")?;
            let desired_free_space_bytes: u64 = size_str.parse()?;

            resize_plan = Some(auto_resize_partition(
                executor,
                disk_info,
                disk_path,
                partition_path,
                desired_free_space_bytes,
            )?);

            let detected = crate::arch::dualboot::detect_disks()
                .context("Failed to refresh disk information after resize")?;
            context.set::<DisksKey>(detected.clone());
            disks = detected;
            disk_info = disks
                .iter()
                .find(|d| d.device == disk_path)
                .context("Selected disk not found after resize")?;
        }
    }

    let mut esp_needs_format = false;
    let esp_path = if let Some(esp) = disk_info.find_reusable_esp() {
        println!(
            "Reusing existing ESP: {} ({})",
            esp.device,
            crate::arch::dualboot::format_size(esp.size_bytes)
        );
        esp.device.clone()
    } else {
        println!("No suitable ESP found (need >= 260MB). Creating a new EFI System Partition...");
        let new_esp = create_esp_partition(disk_path, disk_info, executor)?;
        println!("Created new ESP: {}", new_esp);
        esp_needs_format = true;
        new_esp
    };

    if esp_needs_format {
        let detected = crate::arch::dualboot::detect_disks()
            .context("Failed to refresh disk information after ESP creation")?;
        context.set::<DisksKey>(detected.clone());
        disks = detected;
        disk_info = disks
            .iter()
            .find(|d| d.device == disk_path)
            .context("Selected disk not found after ESP creation")?;
    }

    let preferred_region = if let Some(ref partition_path) = resized_partition {
        match find_next_free_region_after_partition(disk_path, disk_info.size_bytes, partition_path)
        {
            Ok(Some(region)) => Some(region),
            Ok(None) if executor.dry_run => resize_plan.map(|plan| plan.preferred_region),
            Ok(None) => {
                if auto_resize_selected {
                    anyhow::bail!("No free region found after resizing {}", partition_path);
                }
                None
            }
            Err(err) => {
                if auto_resize_selected {
                    return Err(err.context("Failed to locate free space after resizing"));
                }
                None
            }
        }
    } else {
        None
    };

    let available_space = preferred_region
        .as_ref()
        .map(|region| region.size_bytes)
        .unwrap_or(disk_info.max_contiguous_free_space_bytes);
    const GB: u64 = 1024 * 1024 * 1024;
    let mut swap_size_bytes = swap_size_gb * GB;

    if available_space <= crate::arch::dualboot::MIN_LINUX_SIZE {
        anyhow::bail!("Not enough contiguous free space for minimum root");
    }

    let swap_cap_by_ratio = available_space / 3;
    let swap_cap_by_root_min =
        available_space.saturating_sub(crate::arch::dualboot::MIN_LINUX_SIZE);
    let swap_cap = swap_cap_by_ratio.min(swap_cap_by_root_min);

    if swap_size_bytes > swap_cap {
        swap_size_bytes = swap_cap;
        let adjusted_swap_gb = (swap_size_bytes / GB).max(1);
        println!(
            "Capping swap to {} (was {} GiB) to keep swap <= half of root",
            crate::arch::dualboot::format_size(adjusted_swap_gb * GB),
            swap_size_gb
        );
        swap_size_bytes = adjusted_swap_gb * GB;
        swap_size_gb = adjusted_swap_gb;
    }

    let min_required = crate::arch::dualboot::MIN_LINUX_SIZE + swap_size_bytes;
    let alignment_slack = 2 * 1024 * 1024;

    if available_space + alignment_slack < min_required {
        anyhow::bail!(
            "Not enough contiguous free space: {} available, {} required ({} Root + {} Swap)",
            crate::arch::dualboot::format_size(available_space),
            crate::arch::dualboot::format_size(min_required),
            crate::arch::dualboot::format_size(crate::arch::dualboot::MIN_LINUX_SIZE),
            crate::arch::dualboot::format_size(swap_size_bytes)
        );
    }

    let (root_path, swap_path) = create_dualboot_partitions(
        disk_path,
        swap_size_gb,
        disk_info.size_bytes,
        executor,
        preferred_region,
    )?;

    context.set::<DualBootPartitions>(DualBootPartitionPaths {
        root: root_path,
        boot: esp_path.clone(),
        swap: swap_path,
    });

    context.set::<EspNeedsFormat>(esp_needs_format);

    mount::format_and_mount_partitions(context, executor)?;

    Ok(())
}

fn auto_resize_partition(
    executor: &CommandExecutor,
    disk_info: &crate::arch::dualboot::DiskInfo,
    disk_path: &str,
    partition_path: &str,
    desired_free_space_bytes: u64,
) -> Result<ResizePlan> {
    let partition = disk_info
        .partitions
        .iter()
        .find(|p| p.device == partition_path)
        .context("Selected partition not found for resize")?;

    let resize_info = partition
        .resize_info
        .as_ref()
        .context("No resize info for partition")?;

    if !resize_info.can_shrink {
        anyhow::bail!("Selected partition is not shrinkable");
    }

    let min_size_bytes = resize_info
        .min_size_bytes
        .context("Automatic resize requires a known minimum size")?;

    let fs_type = partition
        .filesystem
        .as_ref()
        .map(|f| f.fs_type.as_str())
        .unwrap_or("unknown");

    if !matches!(fs_type, "ntfs" | "ext4" | "ext3" | "ext2") {
        anyhow::bail!(
            "Filesystem {} is not supported for automatic resize",
            fs_type
        );
    }

    let layout = get_partition_layout(disk_path, partition_path)
        .context("Failed to read partition layout")?;

    let existing_free_region =
        find_adjacent_free_region_after_partition(disk_path, disk_info.size_bytes, &layout)?;
    let existing_free_bytes = existing_free_region
        .as_ref()
        .map(|region| region.size_bytes)
        .unwrap_or(0);

    let shrink_bytes = desired_free_space_bytes.saturating_sub(existing_free_bytes);
    if shrink_bytes == 0 {
        let expected_region = FreeRegion {
            start: layout.start + layout.size,
            sectors: existing_free_region
                .as_ref()
                .map(|region| region.sectors)
                .unwrap_or(0),
            size_bytes: existing_free_bytes,
        };

        println!(
            "Skipping resize; existing free space after {} is {}",
            partition_path,
            crate::arch::dualboot::format_size(existing_free_bytes)
        );

        return Ok(ResizePlan {
            preferred_region: expected_region,
        });
    }

    let mut target_size_bytes = partition.size_bytes.saturating_sub(shrink_bytes);
    if target_size_bytes < min_size_bytes {
        anyhow::bail!(
            "Requested resize would shrink below minimum size ({})",
            crate::arch::dualboot::format_size(min_size_bytes)
        );
    }

    let aligned_target_bytes = align_down(target_size_bytes, 1024 * 1024).max(min_size_bytes);
    if aligned_target_bytes >= partition.size_bytes {
        anyhow::bail!("Aligned target size is not smaller than current size");
    }

    target_size_bytes = aligned_target_bytes;

    let freed_bytes = partition.size_bytes.saturating_sub(target_size_bytes);
    let total_expected_free = freed_bytes.saturating_add(existing_free_bytes);

    println!(
        "Resizing {} from {} to {} (freeing {})",
        partition_path,
        crate::arch::dualboot::format_size(partition.size_bytes),
        crate::arch::dualboot::format_size(target_size_bytes),
        crate::arch::dualboot::format_size(freed_bytes)
    );

    if let Some(mount_point) = &partition.mount_point {
        executor.run(Command::new("umount").arg(mount_point))?;
    }

    match fs_type {
        "ntfs" => {
            executor.run(Command::new("ntfsresize").args([
                "--force",
                "--size",
                &target_size_bytes.to_string(),
                partition_path,
            ]))?;
        }
        "ext4" | "ext3" | "ext2" => {
            executor.run(Command::new("e2fsck").args(["-f", partition_path]))?;
            let size_kib = target_size_bytes / 1024;
            executor.run(
                Command::new("resize2fs")
                    .arg(partition_path)
                    .arg(format!("{}K", size_kib)),
            )?;
        }
        _ => {
            anyhow::bail!("Filesystem {} is not supported for resize", fs_type);
        }
    }

    let new_size_sectors = target_size_bytes / layout.sector_size;
    let part_num = parse_partition_number(disk_path, partition_path)?;

    let mut script = format!("size={}", new_size_sectors);
    if let Some(part_type) = partition.partition_type.as_deref() {
        script.push_str(&format!(", type={}", part_type));
    }
    script.push('\n');

    executor.run_with_input(
        Command::new("sfdisk")
            .arg("-N")
            .arg(part_num.to_string())
            .arg(disk_path),
        &script,
    )?;

    if !executor.dry_run {
        executor.run(Command::new("udevadm").arg("settle"))?;
        std::thread::sleep(std::time::Duration::from_secs(2));
    }

    Ok(ResizePlan {
        preferred_region: FreeRegion {
            start: layout.start + new_size_sectors,
            sectors: total_expected_free / layout.sector_size,
            size_bytes: total_expected_free,
        },
    })
}

fn find_adjacent_free_region_after_partition(
    disk_path: &str,
    disk_size_bytes: u64,
    layout: &PartitionLayout,
) -> Result<Option<FreeRegion>> {
    let regions = get_free_regions(disk_path, Some(disk_size_bytes))
        .context("Failed to get free regions for resize")?;

    let partition_end = layout.start + layout.size;
    let alignment_slack = (1024 * 1024 / layout.sector_size).max(1);

    Ok(regions.into_iter().find(|region| {
        region.start >= partition_end && region.start <= partition_end + alignment_slack
    }))
}

fn find_next_free_region_after_partition(
    disk_path: &str,
    disk_size_bytes: u64,
    partition_path: &str,
) -> Result<Option<FreeRegion>> {
    let layout = get_partition_layout(disk_path, partition_path)
        .context("Failed to read partition layout")?;
    let regions = get_free_regions(disk_path, Some(disk_size_bytes))
        .context("Failed to get free regions after resize")?;

    let partition_end = layout.start + layout.size;

    Ok(regions
        .into_iter()
        .filter(|region| region.start >= partition_end)
        .min_by_key(|region| region.start))
}

fn create_esp_partition(
    disk_path: &str,
    disk_info: &crate::arch::dualboot::DiskInfo,
    executor: &CommandExecutor,
) -> Result<String> {
    let partitions_before = get_current_partitions(disk_path)?;

    let esp_size_bytes = MIN_ESP_SIZE;
    let esp_sectors = esp_size_bytes.div_ceil(512);

    let regions = get_free_regions(disk_path, Some(disk_info.size_bytes))
        .context("Failed to get free regions for ESP creation")?;

    let region = regions
        .iter()
        .find(|r| r.sectors >= esp_sectors)
        .context("No free region large enough to create an EFI System Partition (need >= 260MB)")?;

    let start_sector = region.start;

    let type_code = match disk_info.partition_table {
        PartitionTableType::GPT => "c12a7328-f81f-11d2-ba4b-00a0c93ec93b",
        PartitionTableType::MBR | PartitionTableType::Unknown => "0xef",
    };

    let script = format!(
        "start={}, size={}, type={}\n",
        start_sector, esp_sectors, type_code
    );

    executor.run_with_input(
        Command::new("sfdisk").arg("--append").arg(disk_path),
        &script,
    )?;

    if !executor.dry_run {
        executor.run(Command::new("udevadm").arg("settle"))?;
        std::thread::sleep(std::time::Duration::from_secs(2));
    }

    let partitions_after = get_current_partitions(disk_path)?;

    let mut new_parts: Vec<String> = partitions_after
        .into_iter()
        .filter(|p| !partitions_before.contains(p))
        .collect();

    if new_parts.is_empty() {
        anyhow::bail!("Failed to identify newly created ESP partition");
    }

    new_parts.sort_by_key(|p| get_partition_size_bytes(p).unwrap_or(u64::MAX));
    Ok(new_parts[0].clone())
}

fn create_dualboot_partitions(
    disk_path: &str,
    swap_size_gb: u64,
    disk_size_bytes: u64,
    executor: &CommandExecutor,
    preferred_region: Option<FreeRegion>,
) -> Result<(String, String)> {
    println!("Creating partitions in free space (optimal placement)...");

    let partitions_before = get_current_partitions(disk_path)?;

    let regions = if let Some(region) = preferred_region {
        vec![region]
    } else {
        get_free_regions(disk_path, Some(disk_size_bytes))
            .context("Failed to get free space regions")?
    };

    if regions.is_empty() {
        anyhow::bail!("No free space regions detected!");
    }

    let swap_size_bytes = swap_size_gb * 1024 * 1024 * 1024;
    let swap_sectors = swap_size_bytes.div_ceil(512);

    let mut swap_start_sector = 0;
    let mut found_swap = false;

    let mut available_regions = regions.clone();

    for region in available_regions.iter_mut() {
        if region.sectors >= swap_sectors {
            swap_start_sector = region.start;

            region.start += swap_sectors;
            region.sectors -= swap_sectors;
            region.size_bytes = region.size_bytes.saturating_sub(swap_size_bytes);

            found_swap = true;
            break;
        }
    }

    if !found_swap {
        anyhow::bail!(
            "Could not find a contiguous free region large enough for Swap ({} GB)",
            swap_size_gb
        );
    }

    let root_region = available_regions
        .iter()
        .max_by_key(|r| r.sectors)
        .context("No free regions left for Root partition")?;

    let root_start_sector = root_region.start;
    let root_size_sectors = root_region.sectors;

    let root_size_bytes = root_size_sectors * 512;
    if root_size_bytes < crate::arch::dualboot::MIN_LINUX_SIZE {
        anyhow::bail!(
            "Largest remaining free space is too small for Root: {}",
            crate::arch::dualboot::format_size(root_size_bytes)
        );
    }

    println!("Placement:");
    println!(
        "  Swap: Start Sector {}, Size {} GB",
        swap_start_sector, swap_size_gb
    );
    println!(
        "  Root: Start Sector {}, Size {} (approx)",
        root_start_sector,
        crate::arch::dualboot::format_size(root_size_bytes)
    );

    let script = format!(
        "start={}, size={}, type=S\n\
         start={}, size={}, type=L\n",
        swap_start_sector, swap_sectors, root_start_sector, root_size_sectors
    );

    executor.run_with_input(
        Command::new("sfdisk").arg("--append").arg(disk_path),
        &script,
    )?;

    if !executor.dry_run {
        executor.run(Command::new("udevadm").arg("settle"))?;
        std::thread::sleep(std::time::Duration::from_secs(2));
    }

    let partitions_after = get_current_partitions(disk_path)?;

    let new_partitions: Vec<String> = partitions_after
        .into_iter()
        .filter(|p| !partitions_before.contains(p))
        .collect();

    if new_partitions.len() < 2 {
        anyhow::bail!(
            "Expected 2 new partitions, found {}: {:?}",
            new_partitions.len(),
            new_partitions
        );
    }

    let mut swap_path = String::new();
    let root_path: String;

    for p in &new_partitions {
        let size = get_partition_size_bytes(p)?;

        let diff = (size as i64 - swap_size_bytes as i64).abs();
        let margin = (swap_size_bytes / 20) as i64;

        if diff < margin {
            if swap_path.is_empty() {
                swap_path = p.clone();
            }
        }
    }

    if swap_path.is_empty() {
        let mut sorted_by_size = new_partitions.clone();
        sorted_by_size.sort_by_key(|p| get_partition_size_bytes(p).unwrap_or(0));

        swap_path = sorted_by_size[0].clone();
        let assumed_root = sorted_by_size[1].clone();
        root_path = assumed_root.clone();

        println!(
            "Warning: Could not identify partitions by exact size match. Assuming smaller ({}) is Swap and larger ({}) is Root.",
            swap_path, assumed_root
        );
    } else {
        root_path = new_partitions
            .iter()
            .find(|p| **p != swap_path)
            .unwrap()
            .clone();
    }

    let identified_swap_size = get_partition_size_bytes(&swap_path)?;
    println!(
        "Identified Swap: {} ({})",
        swap_path,
        crate::arch::dualboot::format_size(identified_swap_size)
    );
    println!(
        "Identified Root: {} ({})",
        root_path,
        crate::arch::dualboot::format_size(get_partition_size_bytes(&root_path)?)
    );
    println!(
        "Created root partition: {} ({})",
        root_path,
        crate::arch::dualboot::format_size(root_size_bytes)
    );

    Ok((root_path, swap_path))
}
