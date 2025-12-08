use crate::arch::engine::{InstallContext, Question, QuestionId, QuestionResult};
use crate::menu::slide::run_slider;
use crate::menu_utils::{FzfWrapper, SliderConfig};
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

        let result = tokio::task::spawn_blocking(move || -> anyhow::Result<(Vec<crate::arch::dualboot::PartitionInfo>, Vec<crate::arch::dualboot::PartitionInfo>)> {
            let disks = crate::arch::dualboot::detect_disks()?;
            let disk_info = disks
                .iter()
                .find(|d| d.device == disk_path_owned)
                .context("Selected disk not found")?;

            let feasibility = crate::arch::dualboot::check_disk_dualboot_feasibility(disk_info);

            if !feasibility.feasible {
                return Err(anyhow::anyhow!(
                    "Dual boot not feasible: {}",
                    feasibility
                        .reason
                        .unwrap_or_else(|| "Unknown reason".to_string())
                ));
            }

            let shrinkable_partitions: Vec<crate::arch::dualboot::PartitionInfo> = disk_info
                .partitions
                .iter()
                .filter(|p| crate::arch::dualboot::is_dualboot_feasible(p))
                .cloned()
                .collect();

            Ok((shrinkable_partitions.clone(), shrinkable_partitions))
        })
        .await?;

        let (_disk_info_clone, shrinkable_partitions) = result?;

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

        let part_path_owned = part_path.to_string();
        let partition = tokio::task::spawn_blocking(move || {
            let disks = crate::arch::dualboot::detect_disks()?;
            let disk_info = disks
                .iter()
                .find(|d| d.device == disk_path_owned)
                .context("Selected disk not found")?;

            disk_info
                .partitions
                .iter()
                .find(|p| p.device == part_path_owned)
                .cloned()
                .context("Selected partition not found on disk")
        })
        .await??;

        let resize_info = partition
            .resize_info
            .as_ref()
            .context("No resize info for partition")?;

        if !resize_info.can_shrink {
            return Err(anyhow::anyhow!("Partition is not shrinkable"));
        }

        let partition_size = partition.size_bytes;
        let min_existing = resize_info.min_size_bytes.unwrap_or(0);

        // Minimum for Linux: 10GB
        let min_linux = crate::arch::dualboot::MIN_LINUX_SIZE; // 10 GB

        // Calculate available space for Linux (Partition size - Existing OS min)
        let max_linux = partition_size.saturating_sub(min_existing);

        if max_linux < min_linux {
            FzfWrapper::message(&format!(
                "{} Not enough free space on partition for Linux.\nNeed 10GB, but only {} available (after preserving existing OS).",
                NerdFont::Warning,
                crate::arch::dualboot::format_size(max_linux)
            ))?;
            return Ok(QuestionResult::Cancelled);
        }

        // Convert to GB for slider (easier to read/manage)
        const GB: u64 = 1024 * 1024 * 1024;
        let min_gb = min_linux / GB;
        let max_gb = max_linux / GB;
        let default_gb = (min_gb + max_gb) / 2;

        let config = SliderConfig::new(
            min_gb as i64,
            max_gb as i64,
            Some(default_gb as i64),
            Some(1),  // Step 1 GB
            Some(10), // Big step 10 GB
            Some("Linux Size (GB)".to_string()),
            None, // No command to execute on change
        )?;

        // Run slider in sync task since it uses TUI
        let result = tokio::task::spawn_blocking(move || run_slider(config)).await?;

        match result {
            Ok(Some(gb)) => {
                let bytes = gb as u64 * GB;
                Ok(QuestionResult::Answer(bytes.to_string()))
            }
            Ok(None) => Ok(QuestionResult::Cancelled),
            Err(e) => Err(anyhow::anyhow!("Slider failed: {}", e)),
        }
    }
}
