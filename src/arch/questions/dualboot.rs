use crate::arch::engine::{InstallContext, Question, QuestionId, QuestionResult};
use crate::arch::questions::partition::partition_belongs_to_disk;
use crate::common::format::format_size;
use crate::menu::slide::run_slider;
use crate::menu_utils::{FzfPreview, FzfSelectable, FzfWrapper, SliderConfig};
use crate::ui::catppuccin::colors;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;
use anyhow::{Context, Result, bail};

pub struct DualBootPartitionQuestion;

#[derive(Clone)]
struct DualBootPartitionOption {
    info: crate::arch::dualboot::PartitionInfo,
}

impl DualBootPartitionOption {
    fn os_label(&self) -> String {
        self.info
            .detected_os
            .as_ref()
            .map(|os| os.name.clone())
            .unwrap_or_else(|| "Unknown".to_string())
    }

    fn fs_label(&self) -> &str {
        self.info
            .filesystem
            .as_ref()
            .map(|fs| fs.fs_type.as_str())
            .unwrap_or("unknown")
    }

    fn mount_label(&self) -> &str {
        self.info.mount_point.as_deref().unwrap_or("not mounted")
    }
}

impl FzfSelectable for DualBootPartitionOption {
    fn fzf_display_text(&self) -> String {
        format!(
            "{} ({}, {})",
            self.info.device,
            self.info.size_human(),
            self.os_label()
        )
    }

    fn fzf_preview(&self) -> FzfPreview {
        let resize_info = self.info.resize_info.as_ref();
        let can_shrink = resize_info.map(|info| info.can_shrink).unwrap_or(false);
        let resize_status = if can_shrink {
            "Shrinkable"
        } else {
            "Not shrinkable"
        };
        let resize_color = if can_shrink {
            colors::GREEN
        } else {
            colors::YELLOW
        };

        let mut builder = PreviewBuilder::new()
            .header(NerdFont::HardDrive, "Partition Details")
            .subtext("This partition will be resized to make space for Linux.")
            .blank()
            .field("Device", &self.info.device)
            .field("Size", &self.info.size_human())
            .field("Filesystem", self.fs_label())
            .field("Detected OS", &self.os_label())
            .field("Mount", self.mount_label())
            .blank()
            .line(resize_color, None, "Resize Support")
            .field_indented("Status", resize_status);

        if let Some(info) = resize_info {
            if let Some(min_size) = info.min_size_human() {
                builder = builder.field_indented("Min size", &min_size);
            }
            if let Some(reason) = info.reason.as_deref() {
                builder = builder.field_indented("Reason", reason);
            }
            if !info.prerequisites.is_empty() {
                builder = builder
                    .blank()
                    .line(colors::TEAL, None, "Prerequisites")
                    .bullets(info.prerequisites.iter());
            }
        }

        builder.build()
    }

    fn fzf_key(&self) -> String {
        self.info.device.clone()
    }
}

#[async_trait::async_trait]
impl Question for DualBootPartitionQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::DualBootPartition
    }

    fn description(&self) -> Option<&str> {
        Some("Select a partition to resize for Linux")
    }

    fn should_ask(&self, context: &InstallContext) -> bool {
        context
            .get_answer(&QuestionId::PartitioningMethod)
            .map(|s| s.contains("Dual Boot"))
            .unwrap_or(false)
    }

    fn depends_on(&self) -> &[QuestionId] {
        &[QuestionId::Disk, QuestionId::PartitioningMethod]
    }

    fn validate(&self, context: &InstallContext, answer: &str) -> Result<(), String> {
        // The "use existing free space" marker is disk-independent
        if answer == "__free_space__" {
            return Ok(());
        }

        // ask() only offers partitions of the selected disk, so a mismatch
        // means the answer was given for a different disk.
        if let Some(disk) = context.get_answer(&QuestionId::Disk)
            && !partition_belongs_to_disk(answer, disk)
        {
            return Err(format!(
                "Partition {} is not on the selected disk {}",
                answer, disk
            ));
        }

        Ok(())
    }

    async fn ask(&self, context: &InstallContext) -> Result<QuestionResult> {
        // disk_path is now just the device path (e.g., "/dev/sda")
        let disk_path = context
            .get_answer(&QuestionId::Disk)
            .context("No disk selected")?;

        // Get disks from cache or detect
        let disks = if let Some(cached) = context.get::<crate::arch::dualboot::DisksKey>() {
            cached
        } else {
            let detected =
                tokio::task::spawn_blocking(crate::arch::dualboot::detect_disks).await??;
            context.set::<crate::arch::dualboot::DisksKey>(detected.clone());
            detected
        };

        let disk_info = disks
            .iter()
            .find(|d| d.device == *disk_path)
            .context("Selected disk not found")?;

        let feasibility = disk_info.check_disk_dualboot_feasibility();

        if !feasibility.feasible {
            let reason = feasibility
                .reason
                .unwrap_or_else(|| "Unknown reason".to_string());
            bail!("Dual boot not feasible: {reason}");
        }

        let shrinkable_partitions: Vec<crate::arch::dualboot::PartitionInfo> = disk_info
            .partitions
            .iter()
            .filter(|p| p.is_dualboot_feasible())
            .cloned()
            .collect();

        // Check if we already have enough free space
        if shrinkable_partitions.is_empty() {
            let free_space_bytes = disk_info.max_contiguous_free_space_bytes;
            let bitlocker_detected = disk_info.partitions.iter().any(|p| {
                p.filesystem
                    .as_ref()
                    .is_some_and(|fs| fs.fs_type.eq_ignore_ascii_case("bitlocker"))
            });

            if disk_info.has_sufficient_free_space() {
                // No resize needed - disk already has enough free space
                FzfWrapper::message(&format!(
                    "{} No partition resize needed!\n\n\
                     Largest contiguous free region: {}.\n\
                     This is enough for a Linux installation (minimum 10 GB).\n\n\
                     {} Proceeding to installation...",
                    NerdFont::Check,
                    format_size(free_space_bytes),
                    NerdFont::ArrowRight
                ))?;
                return Ok(QuestionResult::Answer("__free_space__".to_string()));
            } else {
                let mut message = format!(
                    "{} No shrinkable partitions found on {} and not enough contiguous free space.\n\
                     Largest contiguous free region: {} (need at least 10 GB)",
                    NerdFont::Warning,
                    disk_path,
                    format_size(free_space_bytes)
                );
                if bitlocker_detected {
                    message.push_str(
                        "\n\nBitLocker-encrypted partitions detected.\n\
                         Disable BitLocker in Windows before attempting dual boot.",
                    );
                }
                message.push_str("\n\nSupported auto-resize filesystems: NTFS, ext4/ext3/ext2.");

                FzfWrapper::message(&message)?;
                return Ok(QuestionResult::Cancelled);
            }
        }

        let options: Vec<DualBootPartitionOption> = shrinkable_partitions
            .into_iter()
            .map(|info| DualBootPartitionOption { info })
            .collect();

        let result = FzfWrapper::builder()
            .header(format!(
                "{} Select Partition to Resize",
                NerdFont::HardDrive
            ))
            .select(options)?;

        match result {
            crate::menu_utils::FzfResult::Selected(option) => {
                Ok(QuestionResult::Answer(option.info.device))
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

    fn description(&self) -> Option<&str> {
        Some("Choose how much space to allocate for Linux")
    }

    fn should_ask(&self, context: &InstallContext) -> bool {
        context
            .get_answer(&QuestionId::PartitioningMethod)
            .map(|s| s.contains("Dual Boot"))
            .unwrap_or(false)
    }

    fn depends_on(&self) -> &[QuestionId] {
        // The size is derived from the partition (or the disk's free space),
        // so any change upstream invalidates it.
        &[
            QuestionId::Disk,
            QuestionId::PartitioningMethod,
            QuestionId::DualBootPartition,
        ]
    }

    async fn ask(&self, context: &InstallContext) -> Result<QuestionResult> {
        let part_path = context
            .get_answer(&QuestionId::DualBootPartition)
            .context("No partition selected")?;

        // Handle free space case - no resize needed
        if part_path == "__free_space__" {
            // disk_path is now just the device path (e.g., "/dev/sda")
            let disk_path = context
                .get_answer(&QuestionId::Disk)
                .context("No disk selected")?;

            let disks = if let Some(cached) = context.get::<crate::arch::dualboot::DisksKey>() {
                cached
            } else {
                let detected =
                    tokio::task::spawn_blocking(crate::arch::dualboot::detect_disks).await??;
                context.set::<crate::arch::dualboot::DisksKey>(detected.clone());
                detected
            };

            let disk_info = disks
                .iter()
                .find(|d| d.device == *disk_path)
                .context("Selected disk not found")?;

            // Return the largest contiguous free space as the Linux size
            return Ok(QuestionResult::Answer(
                disk_info.max_contiguous_free_space_bytes.to_string(),
            ));
        }

        // disk_path is now just the device path (e.g., "/dev/sda")
        let disk_path = context
            .get_answer(&QuestionId::Disk)
            .context("No disk selected")?;

        // Get disks from cache or detect (should be cached by previous question)
        let disks = if let Some(cached) = context.get::<crate::arch::dualboot::DisksKey>() {
            cached
        } else {
            let detected =
                tokio::task::spawn_blocking(crate::arch::dualboot::detect_disks).await??;
            context.set::<crate::arch::dualboot::DisksKey>(detected.clone());
            detected
        };

        let disk_info = disks
            .iter()
            .find(|d| d.device == *disk_path)
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
            bail!("Partition is not shrinkable");
        }

        let partition_size = partition.size_bytes;
        let min_existing = resize_info.min_size_bytes.unwrap_or(0);

        // Minimum for Linux + swap (swap can be capped but stays at least 1GB)
        const GB: u64 = 1024 * 1024 * 1024;
        let min_linux = crate::arch::dualboot::MIN_LINUX_SIZE; // 10 GB
        let min_total = min_linux + GB; // +1GB swap minimum

        // Calculate available space for Linux (Partition size - Existing OS min)
        let max_linux = partition_size.saturating_sub(min_existing);

        if max_linux < min_total {
            FzfWrapper::message(&format!(
                "{} Not enough free space on partition for Linux.\nNeed at least {}, but only {} available (after preserving existing OS).",
                NerdFont::Warning,
                format_size(min_total),
                format_size(max_linux)
            ))?;
            return Ok(QuestionResult::Cancelled);
        }

        // Convert to GB for slider (easier to read/manage)
        let min_gb = min_total.div_ceil(GB);
        let max_gb = max_linux / GB;
        let default_gb = (min_gb + max_gb) / 2;

        let config = SliderConfig::builder()
            .min(min_gb as i64)
            .max(max_gb as i64)
            .value(Some(default_gb as i64))
            .step(Some(1)) // Step 1 GB
            .large_step(Some(10)) // Big step 10 GB
            .label(Some("Linux Size (GB)".to_string()))
            .build()?;

        // Run slider in sync task since it uses TUI
        let result = tokio::task::spawn_blocking(move || run_slider(config)).await?;

        match result {
            Ok(Some(gb)) => {
                let bytes = gb as u64 * GB;
                Ok(QuestionResult::Answer(bytes.to_string()))
            }
            Ok(None) => Ok(QuestionResult::Cancelled),
            Err(e) => Err(e).context("Slider failed")?,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arch::engine::InstallContext;

    fn context_with_disk(disk: &str) -> InstallContext {
        let mut context = InstallContext::new();
        context.set_answer(QuestionId::Disk, disk.to_string());
        context
    }

    #[test]
    fn validate_rejects_partitions_from_another_disk() {
        let context = context_with_disk("/dev/sda");
        let result = DualBootPartitionQuestion.validate(&context, "/dev/nvme0n1p3");
        assert!(result.is_err());
    }

    #[test]
    fn validate_accepts_partitions_on_the_selected_disk() {
        let context = context_with_disk("/dev/nvme0n1");
        assert!(
            DualBootPartitionQuestion
                .validate(&context, "/dev/nvme0n1p3")
                .is_ok()
        );
    }

    #[test]
    fn validate_accepts_the_free_space_marker_for_any_disk() {
        let context = context_with_disk("/dev/sda");
        assert!(
            DualBootPartitionQuestion
                .validate(&context, "__free_space__")
                .is_ok()
        );
    }

    #[test]
    fn validate_tolerates_a_missing_disk_answer() {
        let context = InstallContext::new();
        assert!(
            DualBootPartitionQuestion
                .validate(&context, "/dev/sda2")
                .is_ok()
        );
    }
}
