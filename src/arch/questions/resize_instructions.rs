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

        let options = vec![
            format!("{} I have resized the partition", NerdFont::Check),
            format!("{} Open cfdisk to verify/edit", NerdFont::HardDrive),
            format!("{} Go Back", NerdFont::ArrowLeft),
        ];

        // Loop until user confirms or goes back
        loop {
            // We use select here to show the instructions as a "header" for the menu
            // FzfWrapper might truncate long headers, so we might need to be careful.
            // Or we can print it and then show options.

            // Let's try printing to stdout first for the long text, then showing the menu.
            // But QuestionEngine clears screen.
            // So we put it in the header.

            let result = FzfWrapper::builder()
                .header(&full_message)
                .select(options.clone())?;

            match result {
                crate::menu_utils::FzfResult::Selected(opt) => {
                    if opt.contains("I have resized") {
                        return Ok(QuestionResult::Answer("confirmed".to_string()));
                    } else if opt.contains("Open cfdisk") {
                        // Launch cfdisk
                        let mut cmd = std::process::Command::new("cfdisk");
                        cmd.arg(disk_path);
                        let _ = cmd.status();
                        // Loop continues, allowing them to confirm after cfdisk
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
