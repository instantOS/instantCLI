use crate::arch::engine::{InstallContext, Question, QuestionId, QuestionResult};
use crate::menu_utils::FzfWrapper;
use crate::ui::nerd_font::NerdFont;
use anyhow::{Context, Result};

pub struct ResizeInstructionsQuestion;

#[async_trait::async_trait]
impl Question for ResizeInstructionsQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::DualBootInstructions
    }

    fn should_ask(&self, context: &InstallContext) -> bool {
        context
            .get_answer(&QuestionId::PartitioningMethod)
            .map(|s| s.contains("Dual Boot"))
            .unwrap_or(false)
    }

    async fn ask(&self, context: &InstallContext) -> Result<QuestionResult> {
        let partition_path = context
            .get_answer(&QuestionId::DualBootPartition)
            .context("No partition selected")?;

        let size_str = context
            .get_answer(&QuestionId::DualBootSize)
            .context("No size selected")?;

        let new_linux_size_bytes: u64 = size_str.parse()?;

        // Calculate new size for the existing partition
        // We need to get the current size again to calculate the target size
        // Current size - Linux size = New Size for existing partition
        // But wait, the user selected "Linux Size".
        // So the existing partition needs to be shrunk TO (Current - Linux Size).

        // Let's get the partition details again
        // This is a bit redundant but safe
        let disk_str = context.get_answer(&QuestionId::Disk).context("No disk")?;
        let disk_path = disk_str.split('(').next().unwrap_or(disk_str).trim();
        let disk_path_owned = disk_path.to_string();

        let disks_result = tokio::task::spawn_blocking(crate::arch::dualboot::detect_disks).await?;
        let disks = disks_result?;
        let disk_info = disks
            .iter()
            .find(|d| d.device == disk_path_owned)
            .context("Disk not found")?;
        let partition = disk_info
            .partitions
            .iter()
            .find(|p| p.device == *partition_path)
            .context("Partition not found")?;

        let current_size = partition.size_bytes;
        let target_size = current_size.saturating_sub(new_linux_size_bytes);

        let fs_type = partition
            .filesystem
            .as_ref()
            .map(|f| f.fs_type.clone())
            .unwrap_or_default();

        let target_size_gb = target_size as f64 / 1024.0 / 1024.0 / 1024.0;
        let linux_size_gb = new_linux_size_bytes as f64 / 1024.0 / 1024.0 / 1024.0;

        let instructions = format!(
            "To proceed with Dual Boot, you must manually resize the partition:\n\n\
            {} Partition: {}\n\
            {} Filesystem: {}\n\
            {} Current Size: {}\n\
            {} Target Size: {:.1} GB (shrink by {:.1} GB)\n\n\
            Instructions:\n",
            NerdFont::HardDrive,
            partition_path,
            NerdFont::File,
            fs_type,
            NerdFont::Info,
            partition.size_human(),
            NerdFont::ArrowRight,
            target_size_gb,
            linux_size_gb
        );

        let mut detailed_steps = String::new();

        if fs_type == "ntfs" {
            detailed_steps.push_str(&format!(
                "1. Boot into Windows (Recommended)\n\
                 2. Open 'Disk Management'\n\
                 3. Right-click '{}' (usually C:)\n\
                 4. Select 'Shrink Volume'\n\
                 5. Enter amount to shrink: {:.0} MB\n\
                 6. Click 'Shrink'\n\n\
                 Alternatively, use 'ntfsresize' (Advanced):\n\
                 sudo ntfsresize -s {}G {}",
                partition_path,
                new_linux_size_bytes as f64 / 1024.0 / 1024.0,
                target_size_gb as u64,
                partition_path
            ));
        } else if fs_type.starts_with("ext") {
            detailed_steps.push_str(&format!(
                "1. Unmount the partition:\n   sudo umount {}\n\
                 2. Check filesystem:\n   sudo e2fsck -f {}\n\
                 3. Resize filesystem:\n   sudo resize2fs {} {}G\n\
                 4. Resize partition using 'cfdisk' or 'parted'",
                partition_path, partition_path, partition_path, target_size_gb as u64
            ));
        } else {
            detailed_steps.push_str("Please use a partition manager (like GParted or cfdisk) to resize this partition manually.");
        }

        let full_message = format!(
            "{}
{}",
            instructions, detailed_steps
        );

        // Track original sizes for verification
        let original_partition_size = current_size;
        let original_unpartitioned = disk_info.unpartitioned_space_bytes;

        let options = vec![
            format!("{} I have resized the partition", NerdFont::Check),
            format!("{} Open cfdisk to verify/edit", NerdFont::HardDrive),
            format!("{} Go Back", NerdFont::ArrowLeft),
        ];

        // Loop until user confirms or goes back
        loop {
            let result = FzfWrapper::builder()
                .header(&full_message)
                .select(options.clone())?;

            match result {
                crate::menu_utils::FzfResult::Selected(opt) => {
                    if opt.contains("I have resized") || opt.contains("Open cfdisk") {
                        // Check current partition state
                        let disk_path_check = disk_path_owned.clone();
                        let partition_path_check = partition_path.clone();

                        let verification = tokio::task::spawn_blocking(move || {
                            let disks = crate::arch::dualboot::detect_disks()?;
                            let disk = disks.iter().find(|d| d.device == disk_path_check);

                            if let Some(disk) = disk {
                                let partition = disk
                                    .partitions
                                    .iter()
                                    .find(|p| p.device == partition_path_check);
                                Ok::<_, anyhow::Error>((
                                    partition.map(|p| p.size_bytes),
                                    disk.unpartitioned_space_bytes,
                                ))
                            } else {
                                Ok((None, 0))
                            }
                        })
                        .await??;

                        let (current_partition_size, current_unpartitioned) = verification;

                        // Check if resize appears to have been done
                        let partition_shrunk = current_partition_size
                            .map(|s| s < original_partition_size)
                            .unwrap_or(false);
                        let space_freed = current_unpartitioned > original_unpartitioned;
                        let resize_detected = partition_shrunk || space_freed;

                        if opt.contains("Open cfdisk") {
                            // Launch cfdisk using proper TUI handling
                            let _ =
                                crate::common::terminal::run_tui_program("cfdisk", &[disk_path])
                                    .await;

                            // Re-check after cfdisk
                            let disk_path_check = disk_path_owned.clone();
                            let partition_path_check = partition_path.clone();

                            let post_cfdisk = tokio::task::spawn_blocking(move || {
                                let disks = crate::arch::dualboot::detect_disks()?;
                                let disk = disks.iter().find(|d| d.device == disk_path_check);

                                if let Some(disk) = disk {
                                    let partition = disk
                                        .partitions
                                        .iter()
                                        .find(|p| p.device == partition_path_check);
                                    Ok::<_, anyhow::Error>((
                                        partition.map(|p| {
                                            (
                                                p.size_bytes,
                                                crate::arch::dualboot::format_size(p.size_bytes),
                                            )
                                        }),
                                        disk.unpartitioned_space_bytes,
                                    ))
                                } else {
                                    Ok((None, 0))
                                }
                            })
                            .await??;

                            let (partition_info, new_unpartitioned) = post_cfdisk;
                            let new_partition_shrunk = partition_info
                                .as_ref()
                                .map(|(s, _)| *s < original_partition_size)
                                .unwrap_or(false);
                            let new_space_freed = new_unpartitioned > original_unpartitioned;

                            // Display status message
                            println!();
                            if new_partition_shrunk || new_space_freed {
                                let freed_bytes =
                                    new_unpartitioned.saturating_sub(original_unpartitioned);
                                println!(
                                    "{} Resize detected! Free space increased by {}",
                                    NerdFont::Check,
                                    crate::arch::dualboot::format_size(freed_bytes)
                                );
                                if let Some((_size, human)) = partition_info {
                                    println!(
                                        "   Partition {} is now {} (was {})",
                                        partition_path,
                                        human,
                                        crate::arch::dualboot::format_size(original_partition_size)
                                    );
                                }
                            } else if let Some((_, human)) = partition_info {
                                println!(
                                    "{} No resize detected. Partition {} is still {} (target: {:.1} GB)",
                                    NerdFont::Warning,
                                    partition_path,
                                    human,
                                    target_size_gb
                                );
                                println!("   Make sure to save changes in cfdisk before exiting.");
                            }
                            println!();
                            // Loop continues
                        } else {
                            // User clicked "I have resized"
                            if resize_detected {
                                return Ok(QuestionResult::Answer("confirmed".to_string()));
                            } else {
                                // Warn but allow them to proceed
                                println!();
                                println!(
                                    "{} Warning: No partition resize detected!",
                                    NerdFont::Warning
                                );
                                println!("   The partition still appears to be the original size.");
                                println!("   You may want to go back and resize it first.");
                                println!();

                                // Ask for confirmation
                                let confirm_options = vec![
                                    format!("{} Proceed anyway", NerdFont::ArrowRight),
                                    format!("{} Go back and resize", NerdFont::ArrowLeft),
                                ];

                                let confirm = FzfWrapper::builder()
                                    .header(
                                        "Partition does not appear to have been resized. Proceed?",
                                    )
                                    .select(confirm_options)?;

                                match confirm {
                                    crate::menu_utils::FzfResult::Selected(c)
                                        if c.contains("Proceed") =>
                                    {
                                        return Ok(QuestionResult::Answer("confirmed".to_string()));
                                    }
                                    _ => {
                                        // Continue loop
                                    }
                                }
                            }
                        }
                    } else if opt.contains("Go Back") {
                        return Ok(QuestionResult::Cancelled);
                    }
                }
                crate::menu_utils::FzfResult::Cancelled => return Ok(QuestionResult::Cancelled),
                _ => return Ok(QuestionResult::Cancelled),
            }
        }
    }
}
