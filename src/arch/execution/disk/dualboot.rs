use super::mount;
use super::probe::{get_current_partitions, get_partition_size_bytes};
use super::util::{align_down, parse_partition_number};
use crate::arch::dualboot::parsing::{PartitionLayout, get_free_regions, get_partition_layout};
use crate::arch::dualboot::types::{FreeRegion, MIN_ESP_SIZE, Shrinkability};
use crate::arch::dualboot::{DualBootDisksKey, PartitionTableType};
use crate::arch::engine::{
    BootMode, DualBootPartitionPaths, DualBootPartitions, DualBootTarget, EspNeedsFormat,
    FilesystemPlan, InstallContext,
};
use crate::arch::execution::CommandRunner;
use crate::common::blockdev::{Filesystem, ShrinkTool};
use crate::common::format::format_size;
use anyhow::{Context, Result};
use std::process::Command;

#[derive(Debug)]
struct ResizePlan {
    preferred_region: FreeRegion,
}

/// Replace one disk's entry in the wizard's cache, leaving the rest alone.
///
/// The cache holds every disk so later steps can show the whole machine, but
/// only the disk being installed on can have changed. Re-reading all of them
/// would cost a `sfdisk` per disk to refresh a single entry.
fn refresh_cached_disk(context: &InstallContext, disk: &crate::arch::dualboot::DiskInfo) {
    let mut disks = match context.get::<DualBootDisksKey>() {
        Some(cached) => cached,
        // Nothing cached yet — the wizard seeds this from the disk picker, but
        // `ins arch exec` never runs the wizard. Seed it properly rather than
        // leaving a cache holding one disk where it used to hold all of them.
        None => crate::arch::dualboot::detect_disks().unwrap_or_default(),
    };
    match disks.iter_mut().find(|entry| entry.device == disk.device) {
        Some(entry) => *entry = disk.clone(),
        None => disks.push(disk.clone()),
    }
    context.set::<DualBootDisksKey>(disks);
}

pub fn prepare_dualboot_disk(
    context: &InstallContext,
    filesystem: FilesystemPlan,
    target: &DualBootTarget,
    boot_mode: &BootMode,
    executor: &dyn CommandRunner,
    disk_path: &str,
    mut swap_size_gb: u64,
) -> Result<()> {
    println!("Preparing dual boot installation...");

    // One disk is re-read, not the whole machine: describing a disk runs
    // `sfdisk` on it, so a full pass here would cost a subprocess per disk to
    // answer a question about the one the user picked.
    let mut disk_info = crate::arch::dualboot::detect_disk(disk_path)
        .context("Disk detection data not available and re-detection failed")?
        .context("Selected disk not found in detection data")?;
    refresh_cached_disk(context, &disk_info);

    let mut resized_partition: Option<String> = None;
    let mut resize_plan: Option<ResizePlan> = None;
    let auto_resize_selected = matches!(target, DualBootTarget::ResizeAutomatically { .. });

    match target {
        DualBootTarget::ExistingFreeSpace => {}
        DualBootTarget::ResizedManually { partition } => {
            resized_partition = Some(partition.as_str().to_owned());
        }
        DualBootTarget::ResizeAutomatically {
            partition,
            desired_free_space_bytes,
        } => {
            let partition_path = partition.as_str();
            resized_partition = Some(partition_path.to_owned());
            resize_plan = Some(auto_resize_partition(
                executor,
                &disk_info,
                disk_path,
                partition_path,
                desired_free_space_bytes.bytes(),
            )?);

            disk_info = crate::arch::dualboot::detect_disk(disk_path)
                .context("Failed to refresh disk information after resize")?
                .context("Selected disk not found after resize")?;
            refresh_cached_disk(context, &disk_info);
        }
    }

    let mut esp_needs_format = false;
    let esp_path = if let Some(esp) = disk_info.find_reusable_esp() {
        println!(
            "Reusing existing ESP: {} ({})",
            esp.device,
            format_size(esp.size_bytes)
        );
        esp.device.clone()
    } else {
        println!("No suitable ESP found (need >= 260MB). Creating a new EFI System Partition...");
        let new_esp = create_esp_partition(disk_path, &disk_info, executor)?;
        println!("Created new ESP: {}", new_esp);
        esp_needs_format = true;
        new_esp
    };

    if esp_needs_format {
        disk_info = crate::arch::dualboot::detect_disk(disk_path)
            .context("Failed to refresh disk information after ESP creation")?
            .context("Selected disk not found after ESP creation")?;
        refresh_cached_disk(context, &disk_info);
    }

    let preferred_region = if let Some(ref partition_path) = resized_partition {
        match find_next_free_region(disk_path, disk_info.size_bytes, partition_path) {
            Ok(Some(region)) => Some(region),
            Ok(None) if executor.dry_run() => resize_plan.map(|plan| plan.preferred_region),
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
            format_size(adjusted_swap_gb * GB),
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
            format_size(available_space),
            format_size(min_required),
            format_size(crate::arch::dualboot::MIN_LINUX_SIZE),
            format_size(swap_size_bytes)
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

    mount::format_and_mount_partitions(context, filesystem, None, boot_mode, executor)?;

    Ok(())
}

/// What shrinking a filesystem takes, as commands rather than as a decision
/// buried in the executor loop.
///
/// Split out because the partition *layout* is read with a real `sfdisk`, so a
/// test that drives the whole of `auto_resize_partition` would need a block
/// device. The command sequence does not.
enum ShrinkCommand {
    Unmount(String),
    Run(&'static str, Vec<String>),
}

fn shrink_commands(
    filesystem: &Filesystem,
    mount_point: Option<&str>,
    partition_path: &str,
    target_size_bytes: u64,
) -> Result<Vec<ShrinkCommand>> {
    let support = filesystem.shrink_support();
    let Some(tool) = support.tool() else {
        anyhow::bail!("Filesystem {filesystem} cannot be resized automatically");
    };

    let mut commands = Vec::new();
    if support.requires_unmount()
        && let Some(point) = mount_point
    {
        commands.push(ShrinkCommand::Unmount(point.to_string()));
    }

    let mut run = |program: &'static str, args: Vec<String>| {
        commands.push(ShrinkCommand::Run(program, args));
    };
    match tool {
        ShrinkTool::NtfsResize => run(
            "ntfsresize",
            vec![
                "--force".to_string(),
                "--size".to_string(),
                target_size_bytes.to_string(),
                partition_path.to_string(),
            ],
        ),
        ShrinkTool::Resize2Fs => {
            run("e2fsck", vec!["-f".to_string(), partition_path.to_string()]);
            run(
                "resize2fs",
                vec![
                    partition_path.to_string(),
                    format!("{}K", target_size_bytes / 1024),
                ],
            );
        }
        ShrinkTool::Btrfs => {
            let Some(point) = mount_point else {
                anyhow::bail!(
                    "btrfs can only be resized while mounted, and {partition_path} is not"
                );
            };
            let target_gib = target_size_bytes.div_ceil(1024 * 1024 * 1024);
            println!(
                "Resizing btrfs in place to {target_gib} GiB; this relocates data and is I/O heavy."
            );
            run(
                "btrfs",
                vec![
                    "filesystem".to_string(),
                    "resize".to_string(),
                    format!("-{target_gib}G"),
                    point.to_string(),
                ],
            );
        }
    }
    Ok(commands)
}

fn auto_resize_partition(
    executor: &dyn CommandRunner,
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

    let min_size_bytes = match &resize_info.shrinkability {
        Shrinkability::Shrinkable { min_size_bytes } => *min_size_bytes,
        Shrinkability::MinUnknown { reason } => {
            anyhow::bail!("Cannot auto-resize {partition_path}: {reason}");
        }
        Shrinkability::NotShrinkable { reason } => {
            anyhow::bail!("Cannot auto-resize {partition_path}: {reason}");
        }
    };

    let Some(filesystem) = partition.filesystem.as_ref().map(|f| &f.fs_type) else {
        anyhow::bail!("Partition {partition_path} has no detectable filesystem");
    };
    let support = filesystem.shrink_support();
    if !support.is_supported() {
        anyhow::bail!(
            "Filesystem {filesystem} cannot be resized automatically; \
             install to free space or resize it yourself"
        );
    }

    let layout = get_partition_layout(disk_path, partition_path)
        .context("Failed to read partition layout")?;

    let existing_free_region = find_adjacent_free_region(disk_path, disk_info.size_bytes, &layout)?;
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
            format_size(existing_free_bytes)
        );

        return Ok(ResizePlan {
            preferred_region: expected_region,
        });
    }

    let mut target_size_bytes = partition.size_bytes.saturating_sub(shrink_bytes);
    if target_size_bytes < min_size_bytes {
        anyhow::bail!(
            "Requested resize would shrink below minimum size ({})",
            format_size(min_size_bytes)
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
        format_size(partition.size_bytes),
        format_size(target_size_bytes),
        format_size(freed_bytes)
    );

    // btrfs is the one case that shrinks in place: the filesystem must stay
    // mounted, because the subvolume tree is what `btrfs filesystem resize`
    // operates on. Everything else is unmounted first.
    for command in shrink_commands(
        filesystem,
        partition.mount_point.as_deref(),
        partition_path,
        target_size_bytes,
    )? {
        match command {
            ShrinkCommand::Unmount(point) => executor.run(Command::new("umount").arg(point))?,
            ShrinkCommand::Run(program, args) => {
                executor.run(Command::new(program).args(&args))?;
            }
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

    if !executor.dry_run() {
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

fn find_adjacent_free_region(
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

fn find_next_free_region(
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
    executor: &dyn CommandRunner,
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

    if !executor.dry_run() {
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
    executor: &dyn CommandRunner,
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
            format_size(root_size_bytes)
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
        format_size(root_size_bytes)
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

    if !executor.dry_run() {
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

        if diff < margin && swap_path.is_empty() {
            swap_path = p.clone();
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
        format_size(identified_swap_size)
    );
    println!(
        "Identified Root: {} ({})",
        root_path,
        format_size(get_partition_size_bytes(&root_path)?)
    );
    println!(
        "Created root partition: {} ({})",
        root_path,
        format_size(root_size_bytes)
    );

    Ok((root_path, swap_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The commands a shrink would run, rendered for assertion.
    fn plan(fs: &str, mount_point: Option<&str>) -> Result<String> {
        let commands = shrink_commands(&fs.into(), mount_point, "/dev/vda1", 512 * 1024 * 1024)?;
        Ok(commands
            .iter()
            .map(|command| match command {
                ShrinkCommand::Unmount(point) => format!("umount {point}"),
                ShrinkCommand::Run(program, args) => {
                    format!("{program} {}", args.join(" "))
                }
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    #[test]
    fn btrfs_shrinks_in_place_and_is_never_unmounted() {
        // The regression this fixes: the old `matches!` refused btrfs outright,
        // while the resize module next door could already report it shrinkable.
        // It is also the only filesystem that shrinks while mounted, which is
        // what makes dual-boot possible without a live medium.
        let commands = plan("btrfs", Some("/mnt/dualboot")).unwrap();
        assert!(commands.contains("btrfs filesystem resize"), "{commands}");
        assert!(
            !commands.contains("umount"),
            "must stay mounted: {commands}"
        );
    }

    #[test]
    fn btrfs_needs_a_mount_point_to_shrink() {
        let error = plan("btrfs", None).unwrap_err().to_string();
        assert!(error.contains("only be resized while mounted"), "{error}");
    }

    #[test]
    fn ext_is_unmounted_then_checked_then_resized() {
        let commands = plan("ext4", Some("/mnt/dualboot")).unwrap();
        assert_eq!(
            commands,
            "umount /mnt/dualboot\ne2fsck -f /dev/vda1\nresize2fs /dev/vda1 524288K"
        );
    }

    #[test]
    fn ntfs_uses_ntfsresize_and_never_the_ext_tools() {
        let commands = plan("ntfs", Some("/mnt/dualboot")).unwrap();
        assert!(commands.contains("ntfsresize --force --size"), "{commands}");
        assert!(!commands.contains("e2fsck"), "{commands}");
        assert!(!commands.contains("resize2fs"), "{commands}");
    }

    /// A disk entry for cache tests.
    fn cache_disk(path: &str, size: u64) -> crate::arch::dualboot::DiskInfo {
        crate::arch::dualboot::DiskInfo {
            device: path.to_string(),
            size_bytes: size,
            partition_table: PartitionTableType::GPT,
            partitions: Vec::new(),
            unpartitioned_space_bytes: 0,
            max_contiguous_free_space_bytes: 0,
        }
    }

    #[test]
    fn refreshing_one_disk_leaves_the_rest_of_the_cache_alone() {
        // The cache holds every disk so later steps can show the whole machine.
        // Only the disk being installed on can have changed, so a refresh must
        // replace one entry rather than re-reading every disk.
        let context = InstallContext::new();
        context.set::<DualBootDisksKey>(vec![
            cache_disk("/dev/vda", 100),
            cache_disk("/dev/vdb", 200),
        ]);

        refresh_cached_disk(&context, &cache_disk("/dev/vda", 999));

        let cached = context.get::<DualBootDisksKey>().unwrap();
        assert_eq!(cached.len(), 2, "no disk is added or dropped");
        assert_eq!(cached[0].size_bytes, 999, "the refreshed disk is updated");
        assert_eq!(
            cached[1].size_bytes, 200,
            "an unrelated disk must be left exactly as it was"
        );
    }

    #[test]
    fn xfs_and_the_unknown_have_no_commands_at_all() {
        for name in ["xfs", "vfat", "zfs", "unknown"] {
            let error = plan(name, Some("/mnt/dualboot")).unwrap_err().to_string();
            assert!(
                error.contains("cannot be resized automatically"),
                "{name}: {error}"
            );
        }
    }
}
