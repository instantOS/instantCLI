use crate::arch::engine::{InstallContext, Question, QuestionId, QuestionResult};
use crate::menu_utils::{FzfPreview, FzfSelectable, FzfWrapper, HeaderBuilder};
use crate::preview::{PreviewId, preview_command};
use crate::ui::nerd_font::NerdFont;
use anyhow::{Context, Result};

/// Represents a partition entry with path and size information
#[derive(Clone, Debug)]
pub struct PartitionEntry {
    /// Device path (e.g., /dev/sda1)
    pub path: String,
    /// Human-readable size (e.g., "512M")
    pub size: String,
}

impl PartitionEntry {
    pub fn new(path: String, size: String) -> Self {
        Self { path, size }
    }
}

impl FzfSelectable for PartitionEntry {
    fn fzf_display_text(&self) -> String {
        format!("{} ({})", self.path, self.size)
    }

    fn fzf_preview(&self) -> FzfPreview {
        FzfPreview::Command(preview_command(PreviewId::Partition))
    }

    fn fzf_key(&self) -> String {
        self.path.clone()
    }
}

/// Represents size in megabytes with parsing capabilities
#[derive(Debug, Clone, PartialEq)]
pub struct PartitionSize(u64);

impl PartitionSize {
    /// Create from bytes (converting to MB)
    pub fn from_bytes(bytes: u64) -> Self {
        Self(bytes / (1024 * 1024))
    }

    /// Get the size in megabytes
    pub fn in_mb(&self) -> u64 {
        self.0
    }
}

/// Trait for partition-specific validation
pub trait PartitionValidator: Send + Sync {
    /// Validate partition-specific requirements
    fn validate_partition(
        &self,
        partition_path: &str,
        size: Option<PartitionSize>,
    ) -> Result<(), String>;
}

/// Default partition validator (no special requirements)
pub struct DefaultPartitionValidator;

impl PartitionValidator for DefaultPartitionValidator {
    fn validate_partition(
        &self,
        _partition_path: &str,
        _size: Option<PartitionSize>,
    ) -> Result<(), String> {
        Ok(())
    }
}

/// ESP partition validator with size requirements
pub struct EspPartitionValidator;

impl PartitionValidator for EspPartitionValidator {
    fn validate_partition(
        &self,
        _partition_path: &str,
        size: Option<PartitionSize>,
    ) -> Result<(), String> {
        // ESP partition must be at least 100MB for UEFI systems
        if let Some(size) = size {
            if size.in_mb() < 100 {
                return Err(format!(
                    "ESP partition must be at least 100MB. Current size: {}MB",
                    size.in_mb()
                ));
            }
        } else {
            return Err("Could not determine ESP partition size. Please ensure the partition has a valid size.".to_string());
        }
        Ok(())
    }
}

pub struct PartitionSelectorQuestion {
    pub id: QuestionId,
    pub prompt: String,
    pub icon: NerdFont,
    pub is_optional: bool,
    pub validator: Box<dyn PartitionValidator>,
}

impl PartitionSelectorQuestion {
    pub fn new(
        id: QuestionId,
        prompt: impl Into<String>,
        icon: NerdFont,
        validator: Option<Box<dyn PartitionValidator>>,
    ) -> Self {
        Self {
            id,
            prompt: prompt.into(),
            icon,
            is_optional: false,
            validator: validator.unwrap_or_else(|| Box::new(DefaultPartitionValidator)),
        }
    }

    pub fn optional(mut self) -> Self {
        self.is_optional = true;
        self
    }
}

#[async_trait::async_trait]
impl Question for PartitionSelectorQuestion {
    fn id(&self) -> QuestionId {
        self.id
    }

    fn description(&self) -> Option<&str> {
        Some(&self.prompt)
    }

    fn is_optional(&self) -> bool {
        self.is_optional
    }

    fn should_ask(&self, context: &InstallContext) -> bool {
        context
            .get_answer(&QuestionId::PartitioningMethod)
            .map(|s| s.contains("Manual"))
            .unwrap_or(false)
    }

    fn depends_on(&self) -> &[QuestionId] {
        &[QuestionId::Disk, QuestionId::PartitioningMethod]
    }

    async fn ask(&self, context: &InstallContext) -> Result<QuestionResult> {
        // disk is now just the device path (e.g., "/dev/sda")
        let disk_path = context
            .get_answer(&QuestionId::Disk)
            .context("No disk selected")?;

        // Run lsblk to get partitions on this disk
        // We do this here to get fresh data after cfdisk
        let output = std::process::Command::new("lsblk")
            .args(["-n", "-o", "NAME,SIZE,TYPE", "-r", disk_path])
            .output()
            .context("Failed to run lsblk to get partitions")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut partitions: Vec<PartitionEntry> = Vec::new();

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let name = parts[0];
                let size = parts[1];
                let type_ = parts[2];

                if type_ == "part" {
                    // Full path
                    let path = if name.starts_with('/') {
                        name.to_string()
                    } else {
                        format!("/dev/{}", name)
                    };
                    partitions.push(PartitionEntry::new(path, size.to_string()));
                }
            }
        }

        if partitions.is_empty() {
            FzfWrapper::message(&format!(
                "{} No partitions found on {}.\nDid you save your changes in cfdisk?",
                NerdFont::Warning,
                disk_path
            ))?;
            return Ok(QuestionResult::Cancelled);
        }

        let result = FzfWrapper::builder()
            .header(HeaderBuilder::new(self.icon, &self.prompt).build())
            .select(partitions)?;

        // Store just the path, not the formatted display string
        Ok(QuestionResult::from_selection(result, |entry| entry.path))
    }

    fn validate(&self, context: &InstallContext, answer: &str) -> Result<(), String> {
        // answer is now just the device path (e.g., "/dev/sda1")
        let part_path = answer;
        let current_id = self.id();

        for (id, val) in context.answers() {
            if id == &current_id {
                continue;
            }

            // Check against other partition questions
            // val is now just the device path, no parsing needed
            if matches!(
                id,
                QuestionId::RootPartition
                    | QuestionId::BootPartition
                    | QuestionId::HomePartition
                    | QuestionId::SwapPartition
            ) && part_path == val
            {
                return Err(format!(
                    "Partition {} is already selected for {:?}",
                    part_path, id
                ));
            }
        }

        // Get partition size from lsblk for validation
        let size = get_partition_size(part_path);

        // The partition must live on the currently selected disk. ask() only
        // offers partitions of that disk, so a mismatch means the answer was
        // given for a different disk (e.g. before a review edit).
        if let Some(disk) = context.get_answer(&QuestionId::Disk)
            && !partition_belongs_to_disk(part_path, disk)
        {
            return Err(format!(
                "Partition {} is not on the selected disk {}",
                part_path, disk
            ));
        }

        // Use the injected validator
        self.validator.validate_partition(part_path, size)?;

        Ok(())
    }
}

/// Check whether a partition device path belongs to the given disk.
///
/// Based on Linux device naming: a partition is the disk path plus a numeric
/// suffix, optionally separated by a "p" when the disk name ends in a digit
/// (`/dev/sda1`, `/dev/nvme0n1p1`, `/dev/mmcblk0p2`).
pub fn partition_belongs_to_disk(partition: &str, disk: &str) -> bool {
    let Some(suffix) = partition.strip_prefix(disk) else {
        return false;
    };
    let digits = suffix.strip_prefix('p').unwrap_or(suffix);
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

/// Get partition size from lsblk
fn get_partition_size(partition_path: &str) -> Option<PartitionSize> {
    let output = std::process::Command::new("lsblk")
        .args(["-n", "-o", "SIZE", "-b", partition_path])
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let size_bytes: u64 = stdout.trim().parse().ok()?;
    // Convert bytes to MB
    Some(PartitionSize::from_bytes(size_bytes))
}

#[cfg(test)]
mod tests {
    use super::partition_belongs_to_disk;

    #[test]
    fn sata_partitions_match_their_disk() {
        assert!(partition_belongs_to_disk("/dev/sda1", "/dev/sda"));
        assert!(partition_belongs_to_disk("/dev/sda10", "/dev/sda"));
    }

    #[test]
    fn nvme_partitions_match_their_disk() {
        assert!(partition_belongs_to_disk("/dev/nvme0n1p3", "/dev/nvme0n1"));
    }

    #[test]
    fn mmc_partitions_match_their_disk() {
        assert!(partition_belongs_to_disk("/dev/mmcblk0p2", "/dev/mmcblk0"));
    }

    #[test]
    fn partitions_on_other_disks_do_not_match() {
        assert!(!partition_belongs_to_disk("/dev/nvme0n1p3", "/dev/sda"));
        assert!(!partition_belongs_to_disk("/dev/sdb1", "/dev/sda"));
        assert!(!partition_belongs_to_disk("/dev/sda1", "/dev/nvme0n1"));
    }

    #[test]
    fn whole_disks_and_non_partitions_do_not_match() {
        assert!(!partition_belongs_to_disk("/dev/sda", "/dev/sda"));
        assert!(!partition_belongs_to_disk("/dev/sda", "/dev/sdb"));
        assert!(!partition_belongs_to_disk("/dev/sdaX", "/dev/sda"));
    }

    #[test]
    fn dual_boot_free_space_marker_is_not_a_partition() {
        assert!(!partition_belongs_to_disk("__free_space__", "/dev/sda"));
    }
}
