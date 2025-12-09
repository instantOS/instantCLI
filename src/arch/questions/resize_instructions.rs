use crate::arch::dualboot::ResizeVerifier;
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

        // Get disk and partition info
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

        // Create resize verifier for tracking changes
        let verifier = ResizeVerifier::with_target(disk_info, partition, target_size);

        let fs_type = partition
            .filesystem
            .as_ref()
            .map(|f| f.fs_type.clone())
            .unwrap_or_default();

        let target_size_gb = target_size as f64 / 1024.0 / 1024.0 / 1024.0;
        let linux_size_gb = new_linux_size_bytes as f64 / 1024.0 / 1024.0 / 1024.0;

        // Build instructions message
        let full_message = build_instructions_message(
            partition_path,
            &fs_type,
            partition.size_human(),
            target_size_gb,
            linux_size_gb,
            new_linux_size_bytes,
        );

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
                    if opt.contains("Open cfdisk") {
                        // Launch cfdisk
                        let _ =
                            crate::common::terminal::run_tui_program("cfdisk", &[disk_path]).await;

                        // Check and display status
                        let status = verifier.check_async().await?;
                        display_resize_status(&status);
                        // Loop continues
                    } else if opt.contains("I have resized") {
                        // Check if resize was performed
                        let status = verifier.check_async().await?;

                        if status.resize_detected {
                            return Ok(QuestionResult::Answer("confirmed".to_string()));
                        } else {
                            // Warn and ask for confirmation
                            if confirm_proceed_without_resize(&status)? {
                                return Ok(QuestionResult::Answer("confirmed".to_string()));
                            }
                            // Continue loop
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

/// Build the instructions message based on filesystem type
fn build_instructions_message(
    partition_path: &str,
    fs_type: &str,
    current_size_human: String,
    target_size_gb: f64,
    linux_size_gb: f64,
    linux_size_bytes: u64,
) -> String {
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
        current_size_human,
        NerdFont::ArrowRight,
        target_size_gb,
        linux_size_gb
    );

    let detailed_steps = if fs_type == "ntfs" {
        format!(
            "1. Boot into Windows (Recommended)\n\
             2. Open 'Disk Management'\n\
             3. Right-click '{}' (usually C:)\n\
             4. Select 'Shrink Volume'\n\
             5. Enter amount to shrink: {:.0} MB\n\
             6. Click 'Shrink'\n\n\
             Alternatively, use 'ntfsresize' (Advanced):\n\
             sudo ntfsresize -s {}G {}",
            partition_path,
            linux_size_bytes as f64 / 1024.0 / 1024.0,
            target_size_gb as u64,
            partition_path
        )
    } else if fs_type.starts_with("ext") {
        format!(
            "1. Unmount the partition:\n   sudo umount {}\n\
             2. Check filesystem:\n   sudo e2fsck -f {}\n\
             3. Resize filesystem:\n   sudo resize2fs {} {}G\n\
             4. Resize partition using 'cfdisk' or 'parted'",
            partition_path, partition_path, partition_path, target_size_gb as u64
        )
    } else {
        "Please use a partition manager (like GParted or cfdisk) to resize this partition manually."
            .to_string()
    };

    format!("{}\n{}", instructions, detailed_steps)
}

/// Display the resize status to the user
fn display_resize_status(status: &crate::arch::dualboot::ResizeStatus) {
    println!();
    if status.resize_detected {
        println!("{} {}", NerdFont::Check, status.message);
    } else {
        println!("{} {}", NerdFont::Warning, status.message);
        println!("   Make sure to save changes in cfdisk before exiting.");
    }
    println!();
}

/// Ask user to confirm proceeding without detected resize
fn confirm_proceed_without_resize(status: &crate::arch::dualboot::ResizeStatus) -> Result<bool> {
    println!();
    println!(
        "{} Warning: No partition resize detected!",
        NerdFont::Warning
    );
    println!("   {}", status.message);
    println!("   You may want to go back and resize it first.");
    println!();

    let confirm_options = vec![
        format!("{} Proceed anyway", NerdFont::ArrowRight),
        format!("{} Go back and resize", NerdFont::ArrowLeft),
    ];

    let confirm = FzfWrapper::builder()
        .header("Partition does not appear to have been resized. Proceed?")
        .select(confirm_options)?;

    Ok(matches!(
        confirm,
        crate::menu_utils::FzfResult::Selected(c) if c.contains("Proceed")
    ))
}
