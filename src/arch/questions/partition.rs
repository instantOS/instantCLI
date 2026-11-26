use anyhow::{Context, Result};

use crate::arch::engine::{
    AskPolicy, HomePartition, InstallContext, PartitionPath, StepId, StepOutcome, WizardStep,
};
use crate::common::blockdev::{BlockDevice, Filesystem, load_lsblk};
use crate::common::format::format_size;
use crate::menu_utils::{DialogOutcome, FzfPreview, FzfSelectable, FzfWrapper, HeaderBuilder};
use crate::preview::{PreviewId, preview_command};
use crate::ui::nerd_font::NerdFont;

#[derive(Clone)]
pub(crate) struct PartitionEntry(BlockDevice);

/// Discover the current partitions belonging to a whole-disk device.
pub(crate) fn list_partitions(disk_path: &str) -> Result<Vec<PartitionEntry>> {
    let output = load_lsblk(&["--list", "--", disk_path])
        .with_context(|| format!("Could not read partitions on {disk_path}"))?;
    Ok(partition_entries(output.blockdevices))
}

fn partition_entries(devices: Vec<BlockDevice>) -> Vec<PartitionEntry> {
    devices
        .into_iter()
        .filter(BlockDevice::is_partition)
        .map(PartitionEntry)
        .collect()
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
    pub id: StepId,
    pub prompt: String,
    pub icon: NerdFont,
    pub optional: bool,
    pub validator: Box<dyn PartitionValidator>,
}

impl PartitionSelectorQuestion {
    pub fn new(
        id: StepId,
        prompt: impl Into<String>,
        icon: NerdFont,
        validator: Option<Box<dyn PartitionValidator>>,
    ) -> Self {
        Self {
            id,
            prompt: prompt.into(),
            icon,
            optional: false,
            validator: validator.unwrap_or_else(|| Box::new(DefaultPartitionValidator)),
        }
    }

    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }
}

#[async_trait::async_trait]
impl WizardStep for PartitionSelectorQuestion {
    fn id(&self) -> StepId {
        self.id
    }

    fn description(&self) -> Option<&str> {
        Some(&self.prompt)
    }

    fn ask_policy(&self, _context: &InstallContext) -> AskPolicy {
        if self.optional {
            // Partition selection has no sensible unattended answer.
            AskPolicy::Optional {
                unattended_answer: None,
            }
        } else {
            AskPolicy::Required
        }
    }

    fn is_relevant(&self, context: &InstallContext) -> bool {
        context.partitioning_method() == Some(crate::arch::engine::PartitioningMethod::Manual)
    }

    fn depends_on(&self) -> &[StepId] {
        &[StepId::Disk, StepId::PartitioningMethod, StepId::RunCfdisk]
    }

    async fn run(&self, context: &InstallContext) -> Result<StepOutcome> {
        let disk_path = context
            .get_answer(&StepId::Disk)
            .context("No disk selected")?;

        let partitions = list_partitions(disk_path)?;

        if partitions.is_empty() {
            return Ok(StepOutcome::revisit(
                StepId::RunCfdisk,
                format!(
                    "No partitions were found on {disk_path}. Reopen cfdisk and write a partition layout."
                ),
            ));
        }

        let result = super::select_one_for_step(
            context,
            self,
            FzfWrapper::builder()
                .header(HeaderBuilder::new(self.icon, &self.prompt).build())
                .items(partitions),
        )?;

        // Store just the path, not the formatted display string
        Ok(StepOutcome::from_dialog(result, |entry| entry.0.path()))
    }

    fn validate(&self, context: &InstallContext, answer: &str) -> Result<(), String> {
        let partition = PartitionPath::parse(answer).map_err(|error| error.to_string())?;
        validate_partition_role(context, self.id, partition.as_str())?;
        self.validator
            .validate_partition(partition.as_str(), get_partition_size(partition.as_str()))
    }
}

fn validate_partition_role(
    context: &InstallContext,
    current_id: StepId,
    path: &str,
) -> Result<(), String> {
    if let Some(id) = assigned_role(context, current_id, path) {
        return Err(format!("Partition {path} is already selected for {id:?}"));
    }

    if let Some(disk) = context.get_answer(&StepId::Disk)
        && !partition_belongs_to_disk(path, disk)
    {
        return Err(format!(
            "Partition {path} is not on the selected disk {disk}"
        ));
    }
    Ok(())
}

fn assigned_role(context: &InstallContext, current_id: StepId, path: &str) -> Option<StepId> {
    context.answers().find_map(|(&id, answer)| {
        if id == current_id {
            return None;
        }
        let matches = match id {
            StepId::RootPartition | StepId::BootPartition | StepId::SwapPartition => path == answer,
            StepId::HomePartition => path == answer.strip_prefix("keep:").unwrap_or(answer),
            _ => false,
        };
        matches.then_some(id)
    })
}

pub(crate) struct HomePartitionQuestion;

#[derive(Clone, Copy)]
enum HomeChoice {
    Root,
    Keep,
    Format,
}

impl FzfSelectable for HomeChoice {
    fn fzf_display_text(&self) -> String {
        match self {
            Self::Root => "Use space on root",
            Self::Keep => "Keep an existing partition",
            Self::Format => "Format a partition as ext4",
        }
        .to_owned()
    }
}

impl FzfSelectable for PartitionEntry {
    fn fzf_display_text(&self) -> String {
        let filesystem = self
            .0
            .fstype
            .as_ref()
            .map_or("no filesystem", Filesystem::as_str);
        let mut label = String::new();
        for character in self.0.label.as_deref().unwrap_or("no label").chars() {
            if character.is_control() {
                label.extend(character.escape_default());
            } else {
                label.push(character);
            }
        }
        let size = self
            .0
            .size
            .map_or_else(|| "unknown size".to_owned(), format_size);
        format!("{}  |  {size}  |  {filesystem}  |  {label}", self.0.path())
    }

    fn fzf_key(&self) -> String {
        self.0.path()
    }

    fn fzf_preview(&self) -> FzfPreview {
        FzfPreview::Command(preview_command(PreviewId::Partition))
    }
}

#[async_trait::async_trait]
impl WizardStep for HomePartitionQuestion {
    fn id(&self) -> StepId {
        StepId::HomePartition
    }

    fn description(&self) -> Option<&str> {
        Some("Choose where to store /home")
    }

    fn is_relevant(&self, context: &InstallContext) -> bool {
        context.partitioning_method() == Some(crate::arch::engine::PartitioningMethod::Manual)
    }

    fn depends_on(&self) -> &[StepId] {
        &[StepId::Disk, StepId::PartitioningMethod, StepId::RunCfdisk]
    }

    async fn run(&self, context: &InstallContext) -> Result<StepOutcome> {
        let disk = context
            .get_answer(&StepId::Disk)
            .context("No disk selected")?;
        let previous = context
            .previous_answer(&self.id())
            .and_then(|answer| HomePartition::from_answer(answer).ok().flatten());
        let mut index = match previous {
            Some(HomePartition::Keep(_)) => 1,
            Some(HomePartition::Format(_)) => 2,
            None => 0,
        };

        loop {
            let choice = FzfWrapper::builder()
                .header(HeaderBuilder::new(NerdFont::Home, "Where should /home live?").build())
                .items(vec![HomeChoice::Root, HomeChoice::Keep, HomeChoice::Format])
                .initial_index(index)
                .select_one()?;
            let keep = match choice {
                DialogOutcome::Submitted(HomeChoice::Root) => {
                    return Ok(StepOutcome::Answer("root".to_owned()));
                }
                DialogOutcome::Submitted(HomeChoice::Keep) => true,
                DialogOutcome::Submitted(HomeChoice::Format) => false,
                DialogOutcome::Cancelled => return Ok(StepOutcome::Pause),
            };
            index = if keep { 1 } else { 2 };
            let candidates: Vec<_> = list_partitions(disk)?
                .into_iter()
                .filter(|entry| assigned_role(context, self.id(), &entry.0.path()).is_none())
                .filter(|entry| {
                    !keep
                        || entry
                            .0
                            .fstype
                            .as_ref()
                            .is_some_and(Filesystem::supports_home_reuse)
                })
                .collect();
            if candidates.is_empty() {
                let message = if keep {
                    "No unassigned ext2, ext3, ext4, or XFS partition is available to keep as /home."
                } else {
                    "No unassigned partition is available to format as /home."
                };
                FzfWrapper::message(message)?;
                continue;
            }

            let prompt = if keep {
                "Keep and mount as /home"
            } else {
                "Erase and format as /home"
            };
            let initial = previous.as_ref().and_then(|home| {
                candidates
                    .iter()
                    .position(|candidate| candidate.0.path() == home.path().as_str())
            });
            let picker = FzfWrapper::builder()
                .header(HeaderBuilder::new(NerdFont::Home, prompt).build())
                .items(candidates)
                .initial_index(initial.unwrap_or(0));
            if let DialogOutcome::Submitted(candidate) = picker.select_one_with_back()? {
                let path = candidate.0.path();
                return Ok(StepOutcome::Answer(if keep {
                    format!("keep:{path}")
                } else {
                    path
                }));
            }
        }
    }

    fn validate(&self, context: &InstallContext, answer: &str) -> Result<(), String> {
        if let Some(home) = HomePartition::from_answer(answer).map_err(|error| error.to_string())? {
            validate_partition_role(context, self.id(), home.path().as_str())?;
        }
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

fn get_partition_size(partition_path: &str) -> Option<PartitionSize> {
    let output = std::process::Command::new("lsblk")
        .args(["-n", "-o", "SIZE", "-b", partition_path])
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let size_bytes: u64 = stdout.trim().parse().ok()?;
    Some(PartitionSize::from_bytes(size_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_partition_metadata_from_lsblk() {
        let output: crate::common::blockdev::LsblkOutput = serde_json::from_str(
            r#"{
            "blockdevices": [
                {"name":"vda","size":21474836480,"type":"disk"},
                {"name":"vda1","size":536870912,"type":"part"},
                {"name":"vda2","size":20937965568,"type":"part","fstype":"ext4","label":"My Home"}
            ]
        }"#,
        )
        .unwrap();
        let partitions = partition_entries(output.blockdevices);
        assert_eq!(partitions.len(), 2);
        assert_eq!(partitions[0].fzf_key(), "/dev/vda1");
        assert_eq!(
            partitions[1].fzf_display_text(),
            "/dev/vda2  |  19.5 GB  |  ext4  |  My Home",
        );
    }

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
        assert!(!partition_belongs_to_disk(
            crate::arch::engine::FREE_SPACE_ANSWER,
            "/dev/sda"
        ));
    }

    #[test]
    fn partition_selector_rejects_unsafe_paths() {
        let question = PartitionSelectorQuestion::new(
            StepId::RootPartition,
            "Select Root Partition",
            NerdFont::HardDrive,
            None,
        );
        let context = InstallContext::new();
        assert!(question.validate(&context, "/dev/../etc/passwd").is_err());
        assert!(question.validate(&context, "not-dev").is_err());
    }

    #[test]
    fn kept_home_cannot_share_a_partition_with_a_format_role() {
        let mut context = InstallContext::new();
        context.set_answer(StepId::Disk, "/dev/sda".to_owned());
        context.set_answer(StepId::RootPartition, "/dev/sda2".to_owned());
        let question = HomePartitionQuestion;
        assert!(question.validate(&context, "keep:/dev/sda2").is_err());
        assert!(question.validate(&context, "keep:/dev/sdb3").is_err());
        assert!(
            question
                .validate(&context, "keep:/dev/../etc/passwd")
                .is_err()
        );
        assert!(question.validate(&context, "keep:/dev/sda3").is_ok());

        context.set_answer(StepId::HomePartition, "keep:/dev/sda3".to_owned());
        assert!(validate_partition_role(&context, StepId::BootPartition, "/dev/sda3").is_err());
    }
}
