use crate::arch::engine::{InstallContext, Question, QuestionId, QuestionResult};
use crate::menu_utils::FzfWrapper;
use crate::ui::nerd_font::NerdFont;
use anyhow::{Context, Result};

/// Represents size in megabytes with parsing capabilities
#[derive(Debug, Clone, PartialEq)]
pub struct PartitionSize(u64);

impl PartitionSize {
    /// Parse a size string (e.g., "512M", "1G", "100MB") and return size in MB
    pub fn parse(size_str: &str) -> Option<Self> {
        if size_str.is_empty() {
            return None;
        }

        let size_str = size_str.trim().to_uppercase();

        // Remove any non-alphanumeric characters except digits and common size indicators
        let cleaned: String = size_str
            .chars()
            .filter(|c| c.is_ascii_digit() || c.is_ascii_alphabetic() || c.is_ascii_whitespace())
            .collect();

        // Try to parse with common suffixes
        if cleaned.ends_with("MB") || cleaned.ends_with("M") {
            if let Ok(size) = cleaned
                .trim_end_matches(|c: char| !c.is_ascii_digit())
                .parse::<u64>()
            {
                return Some(Self(size));
            }
        } else if cleaned.ends_with("GB") || cleaned.ends_with("G") {
            if let Ok(size) = cleaned
                .trim_end_matches(|c: char| !c.is_ascii_digit())
                .parse::<u64>()
            {
                return Some(Self(size * 1024));
            }
        } else if cleaned.ends_with("TB") || cleaned.ends_with("T") {
            if let Ok(size) = cleaned
                .trim_end_matches(|c: char| !c.is_ascii_digit())
                .parse::<u64>()
            {
                return Some(Self(size * 1024 * 1024));
            }
        } else if cleaned.ends_with("KB") || cleaned.ends_with("K") {
            if let Ok(size) = cleaned
                .trim_end_matches(|c: char| !c.is_ascii_digit())
                .parse::<u64>()
            {
                // Convert KB to MB, rounding up
                return Some(Self((size + 1023) / 1024));
            }
        } else {
            // Try to parse as raw number (assume MB)
            if let Ok(size) = size_str.parse::<u64>() {
                return Some(Self(size));
            }
        }

        None
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
        self.id.clone()
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

    async fn ask(&self, context: &InstallContext) -> Result<QuestionResult> {
        let disk = context
            .get_answer(&QuestionId::Disk)
            .context("No disk selected")?;
        let disk_path = disk.split('(').next().unwrap_or(disk).trim();

        // Run lsblk to get partitions on this disk
        // We do this here to get fresh data after cfdisk
        let output = std::process::Command::new("lsblk")
            .args(["-n", "-o", "NAME,SIZE,TYPE", "-r", disk_path])
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut partitions = Vec::new();

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let name = parts[0];
                let size = parts[1];
                let type_ = parts[2];

                if type_ == "part" {
                    // Full path
                    let path = if name.starts_with("/") {
                        name.to_string()
                    } else {
                        format!("/dev/{}", name)
                    };
                    partitions.push(format!("{} ({})", path, size));
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
            .header(format!("{} {}", self.icon, self.prompt))
            .select(partitions)?;

        match result {
            crate::menu_utils::FzfResult::Selected(s) => Ok(QuestionResult::Answer(s)),
            crate::menu_utils::FzfResult::Cancelled => Ok(QuestionResult::Cancelled),
            _ => Ok(QuestionResult::Cancelled),
        }
    }

    fn validate(&self, context: &InstallContext, answer: &str) -> Result<(), String> {
        // Check if this partition is already used by another answer
        let current_id = self.id();
        let part_path = answer.split('(').next().unwrap_or(answer).trim();

        for (id, val) in &context.answers {
            if id == &current_id {
                continue;
            }

            // Check against other partition questions
            if matches!(
                id,
                QuestionId::RootPartition
                    | QuestionId::BootPartition
                    | QuestionId::HomePartition
                    | QuestionId::SwapPartition
            ) {
                let other_path = val.split('(').next().unwrap_or(val).trim();
                if part_path == other_path {
                    return Err(format!(
                        "Partition {} is already selected for {:?}",
                        part_path, id
                    ));
                }
            }
        }

        // Extract size from the partition description (e.g., "/dev/sda1 (512M)")
        let size_str = answer
            .split('(')
            .nth(1)
            .and_then(|s| s.split(')').next())
            .unwrap_or("");
        let size = PartitionSize::parse(size_str.trim());

        // Use the injected validator
        self.validator.validate_partition(part_path, size)?;

        Ok(())
    }
}
