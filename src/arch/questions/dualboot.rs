use crate::arch::engine::{InstallContext, Question, QuestionId, QuestionResult};
use crate::menu_utils::FzfWrapper;
use crate::ui::nerd_font::NerdFont;
use anyhow::{Context, Result};

pub struct DualBootPartitionQuestion;

#[async_trait::async_trait]
impl Question for DualBootPartitionQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::DualBootPartition
    }

    fn should_ask(&self, context: &InstallContext) -> bool {
        context
            .get_answer(&QuestionId::PartitioningMethod)
            .map(|s| s.contains("Dual Boot"))
            .unwrap_or(false)
    }

    async fn ask(&self, context: &InstallContext) -> Result<QuestionResult> {
        let disk_str = context
            .get_answer(&QuestionId::Disk)
            .context("No disk selected")?;
        let disk_path = disk_str.split('(').next().unwrap_or(disk_str).trim();
        let disk_path_owned = disk_path.to_string();

        let disks_result =
            tokio::task::spawn_blocking(move || crate::arch::dualboot::detect_disks()).await?;
        let disks = disks_result?;

        let disk_info = disks
            .iter()
            .find(|d| d.device == disk_path_owned)
            .context("Selected disk not found")?;

        let shrinkable_partitions: Vec<_> = disk_info
            .partitions
            .iter()
            .filter(|p| {
                !p.is_efi
                    && p.resize_info
                        .as_ref()
                        .map(|r| r.can_shrink)
                        .unwrap_or(false)
            })
            .collect();

        if shrinkable_partitions.is_empty() {
            FzfWrapper::message(&format!(
                "{} No shrinkable partitions found on {}.",
                NerdFont::Warning,
                disk_path
            ))?;
            return Ok(QuestionResult::Cancelled);
        }

        let options: Vec<String> = shrinkable_partitions
            .iter()
            .map(|p| {
                let name = p.device.clone();
                let size = p.size_human();
                let os = p
                    .detected_os
                    .as_ref()
                    .map(|o| o.name.clone())
                    .unwrap_or("Unknown".to_string());
                format!("{} ({}, {})", name, size, os)
            })
            .collect();

        let result = FzfWrapper::builder()
            .header(format!(
                "{} Select Partition to Resize",
                NerdFont::HardDrive
            ))
            .select(options)?;

        match result {
            crate::menu_utils::FzfResult::Selected(s) => {
                // Extract device path
                let device = s.split_whitespace().next().unwrap_or(&s).to_string();
                Ok(QuestionResult::Answer(device))
            }
            crate::menu_utils::FzfResult::Cancelled => Ok(QuestionResult::Cancelled),
            _ => Ok(QuestionResult::Cancelled),
        }
    }
}

pub struct DualBootSizeQuestion;

#[async_trait::async_trait]
impl Question for DualBootSizeQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::DualBootSize
    }

    fn should_ask(&self, context: &InstallContext) -> bool {
        context
            .get_answer(&QuestionId::PartitioningMethod)
            .map(|s| s.contains("Dual Boot"))
            .unwrap_or(false)
    }

    async fn ask(&self, context: &InstallContext) -> Result<QuestionResult> {
        let part_path = context
            .get_answer(&QuestionId::DualBootPartition)
            .context("No partition selected")?;

        let disk_str = context
            .get_answer(&QuestionId::Disk)
            .context("No disk selected")?;
        let disk_path = disk_str.split('(').next().unwrap_or(disk_str).trim();
        let disk_path_owned = disk_path.to_string();

        let disks_result =
            tokio::task::spawn_blocking(move || crate::arch::dualboot::detect_disks()).await?;
        let disks = disks_result?;

        let disk_info = disks
            .iter()
            .find(|d| d.device == disk_path_owned)
            .context("Selected disk not found")?;

        let partition = disk_info
            .partitions
            .iter()
            .find(|p| p.device == *part_path)
            .context("Selected partition not found on disk")?;

        let resize_info = partition
            .resize_info
            .as_ref()
            .context("No resize info for partition")?;

        if !resize_info.can_shrink {
            return Err(anyhow::anyhow!("Partition is not shrinkable"));
        }

        let partition_size = partition.size_bytes;
        let min_existing = resize_info.min_size_bytes.unwrap_or(0);

        // Minimum for Linux: 20GB
        let min_linux = 20 * 1024 * 1024 * 1024; // 20 GB

        let partition_size_val = partition_size;
        let min_existing_val = min_existing;
        let min_linux_val = min_linux;

        let size_bytes_result = tokio::task::spawn_blocking(move || {
            crate::arch::dualboot::show_allocation_slider(
                partition_size_val,
                min_existing_val,
                min_linux_val,
            )
        })
        .await?;

        match size_bytes_result {
            Ok(size_bytes) => Ok(QuestionResult::Answer(size_bytes.to_string())),
            Err(e) => {
                // If the user cancelled, we return Cancelled
                if e.to_string().contains("cancelled") {
                    Ok(QuestionResult::Cancelled)
                } else {
                    Err(e)
                }
            }
        }
    }
}
