use crate::arch::engine::{DataKey, InstallContext, StepId, StepOutcome, WizardStep};
use crate::menu_utils::{
    ConfirmResult, FzfPreview, FzfSelectable, FzfWrapper, HeaderBuilder, MenuPresentation,
};
use crate::ui::catppuccin::colors;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;
use anyhow::{Context, Result};

pub struct PrepareDiskStep;

#[async_trait::async_trait]
impl WizardStep for PrepareDiskStep {
    fn id(&self) -> StepId {
        StepId::PrepareDisk
    }

    fn description(&self) -> Option<&str> {
        Some("Prepare the selected disk for exclusive installer access")
    }

    fn depends_on(&self) -> &[StepId] {
        &[StepId::Disk]
    }

    fn completion_is_current(&self, context: &InstallContext) -> bool {
        let Some(device_name) = context.get_answer(&StepId::Disk) else {
            return false;
        };
        crate::arch::disks::get_mounted_partitions(device_name).is_ok_and(|parts| parts.is_empty())
            && crate::arch::disks::get_swap_partitions(device_name)
                .is_ok_and(|parts| parts.is_empty())
    }

    async fn run(&self, context: &InstallContext) -> Result<StepOutcome> {
        let device_name = context
            .get_answer(&StepId::Disk)
            .context("No disk selected")?;
        let mounted = crate::arch::disks::get_mounted_partitions(device_name)
            .context("Failed to inspect mounted partitions")?;
        let swap = crate::arch::disks::get_swap_partitions(device_name)
            .context("Failed to inspect active swap partitions")?;

        if mounted.is_empty() && swap.is_empty() {
            return Ok(StepOutcome::Completed);
        }

        // Build a detailed confirmation message explaining what is in use and why.
        let mut details =
            format!("The selected disk {device_name} has active mounts or swap partitions.\n\n");

        for part in &mounted {
            details.push_str(&format!("  {part}  (mounted)\n"));
        }
        for part in &swap {
            details.push_str(&format!("  {part}  (swap active)\n"));
        }

        details.push_str(
            "\nThe installer needs exclusive access to this disk. It will unmount the partition(s)\nand disable swap so the disk can be safely repartitioned.",
        );

        let confirmed = FzfWrapper::builder()
            .confirm(&details)
            .title(format!("Disk {device_name} is in use"))
            .yes_text("Unmount and continue")
            .no_text("Go back")
            .confirm_dialog()?;

        match confirmed {
            ConfirmResult::Yes => {}
            ConfirmResult::No => return Ok(StepOutcome::back()),
            ConfirmResult::Cancelled => return Ok(StepOutcome::Pause),
        }
        // Prepare the disk
        match crate::arch::disks::prepare_disk(device_name) {
            Ok(result) => {
                if !result.unmounted.is_empty() {
                    println!(
                        "{} Unmounted {} partition(s)",
                        NerdFont::Check,
                        result.unmounted.len()
                    );
                }
                if !result.swapoff.is_empty() {
                    println!(
                        "{} Disabled swap on {} partition(s)",
                        NerdFont::Check,
                        result.swapoff.len()
                    );
                }
                Ok(StepOutcome::Completed)
            }
            Err(e) => Ok(StepOutcome::Retry(format!(
                "Failed to prepare disk {}:\n{}\n\nPlease prepare the disk manually and try again.",
                device_name, e
            ))),
        }
    }
}

#[derive(Clone)]
enum DiskSelection {
    Detected(crate::arch::disks::DiskEntry),
    CustomPath,
}

impl DiskSelection {
    fn custom_preview() -> FzfPreview {
        PreviewBuilder::new()
            .header(NerdFont::Edit, "Custom Disk Path")
            .subtext("Type a disk device path manually when it is not listed.")
            .blank()
            .line(colors::TEAL, None, "Examples")
            .bullets(["/dev/nvme0n1", "/dev/sda"])
            .blank()
            .line(colors::YELLOW, Some(NerdFont::Warning), "Important")
            .bullet("Use a whole disk, not a partition (avoid /dev/sda1).")
            .build()
    }
}

impl FzfSelectable for DiskSelection {
    fn fzf_display_text(&self) -> String {
        match self {
            DiskSelection::Detected(disk) => disk.fzf_display_text(),
            DiskSelection::CustomPath => {
                format!("{} Enter custom disk path", NerdFont::Edit)
            }
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            DiskSelection::Detected(disk) => disk.fzf_preview(),
            DiskSelection::CustomPath => Self::custom_preview(),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            DiskSelection::Detected(disk) => disk.fzf_key(),
            DiskSelection::CustomPath => "custom".to_string(),
        }
    }
}

pub struct DiskQuestion;

impl DiskQuestion {
    fn prompt_custom_disk_path(
        &self,
        context: &InstallContext,
        last_custom_path: &mut Option<String>,
    ) -> Result<Option<String>> {
        loop {
            let mut base = FzfWrapper::builder().prompt("Custom disk path");

            if let Some(previous) = last_custom_path.as_ref() {
                base = base.query(previous.clone());
            }

            let builder = base.input().ghost("/dev/nvme0n1");

            let input = builder.input_dialog()?;
            let path = match input {
                crate::menu_utils::DialogOutcome::Submitted(value) => value.trim().to_string(),
                crate::menu_utils::DialogOutcome::Cancelled => return Ok(None),
            };

            if path.is_empty() {
                FzfWrapper::message("Disk path cannot be empty.")?;
                continue;
            }

            if let Err(message) = self.validate(context, &path) {
                FzfWrapper::message(&format!("{} {}", NerdFont::Warning, message))?;
                continue;
            }

            *last_custom_path = Some(path.clone());

            return Ok(Some(path));
        }
    }
}

#[async_trait::async_trait]
impl WizardStep for DiskQuestion {
    fn id(&self) -> StepId {
        StepId::Disk
    }

    fn description(&self) -> Option<&str> {
        Some("Select the disk for installation")
    }

    fn required_data_keys(&self) -> Vec<String> {
        vec![crate::arch::disks::DisksKey::KEY.to_string()]
    }

    async fn run(&self, context: &InstallContext) -> Result<StepOutcome> {
        let disks = context
            .get::<crate::arch::disks::DisksKey>()
            .unwrap_or_default();

        let mut selections: Vec<DiskSelection> =
            disks.into_iter().map(DiskSelection::Detected).collect();
        let has_detected = !selections.is_empty();
        selections.push(DiskSelection::CustomPath);

        if !has_detected {
            FzfWrapper::message(
                "No disks were detected automatically. You can enter a custom disk path to continue.",
            )?;
        }

        let mut last_custom_path: Option<String> = None;

        loop {
            let result = FzfWrapper::builder()
                .header(HeaderBuilder::new(NerdFont::HardDrive, "Select Installation Disk").build())
                .select_one(selections.clone())?;

            let selection = match result {
                crate::menu_utils::DialogOutcome::Submitted(d) => d,
                crate::menu_utils::DialogOutcome::Cancelled => return Ok(StepOutcome::Pause),
            };

            match selection {
                DiskSelection::Detected(disk) => {
                    return Ok(StepOutcome::Answer(disk.path));
                }
                DiskSelection::CustomPath => {
                    if let Some(path) =
                        self.prompt_custom_disk_path(context, &mut last_custom_path)?
                    {
                        return Ok(StepOutcome::Answer(path));
                    }
                }
            }
        }
    }

    fn validate(&self, _context: &InstallContext, answer: &str) -> Result<(), String> {
        if answer.is_empty() {
            return Err("You must select a disk.".to_string());
        }
        if !answer.starts_with("/dev/") {
            return Err("Invalid disk selection: must start with /dev/".to_string());
        }

        // answer is now just the device path (e.g., "/dev/sda")
        let device_name = answer;

        // Prevent selecting the current root/boot disk
        if let Ok(Some(root_device)) = crate::arch::disks::get_root_device()
            && device_name == root_device
        {
            return Err(format!(
                "Cannot select the current root filesystem device ({}) for installation.\n\
                    This device contains the currently running system and would cause data loss.\n\
                    Please select a different disk.",
                root_device
            ));
        }

        if let Ok(Some(boot_disk)) = crate::arch::disks::get_boot_disk()
            && device_name == boot_disk
        {
            return Err(format!(
                "Cannot select the current boot disk ({}) for installation.\n\
                    This disk contains the currently running system and would cause data loss.\n\
                    Please select a different disk.",
                boot_disk
            ));
        }

        // Note: mounted partitions and swap are now handled interactively in ask()
        // with an offer to automatically prepare the disk

        Ok(())
    }

    fn data_providers(&self) -> Vec<Box<dyn crate::arch::engine::AsyncDataProvider>> {
        vec![Box::new(crate::arch::disks::DiskProvider)]
    }

    fn fatal_error_message(&self, _context: &InstallContext) -> Option<String> {
        None
    }
}

pub struct PartitioningMethodQuestion;

#[derive(Clone)]
enum PartitioningMethodOption {
    Automatic,
    DualBoot,
    Manual,
}

impl PartitioningMethodOption {
    fn label(&self) -> &'static str {
        match self {
            PartitioningMethodOption::Automatic => "Automatic (Erase Disk)",
            PartitioningMethodOption::DualBoot => "Dual Boot (Automatic)",
            PartitioningMethodOption::Manual => "Manual (cfdisk)",
        }
    }

    fn preview(&self) -> FzfPreview {
        match self {
            PartitioningMethodOption::Automatic => PreviewBuilder::new()
                .header(NerdFont::HardDrive, "Automatic Partitioning")
                .subtext("Erase the selected disk and create a recommended layout.")
                .blank()
                .line(colors::TEAL, None, "Best for")
                .bullets([
                    "Fresh installs with no data to keep",
                    "Fast setup with sensible defaults",
                ])
                .blank()
                .line(colors::YELLOW, None, "Warning")
                .bullet("All data on the disk will be lost")
                .build(),
            PartitioningMethodOption::DualBoot => PreviewBuilder::new()
                .header(NerdFont::HardDrive, "Dual Boot")
                .subtext("Shrink an existing partition and create Linux partitions automatically.")
                .blank()
                .line(colors::TEAL, None, "Keeps")
                .bullets(["Existing OS installation", "User data on other partitions"])
                .blank()
                .line(colors::YELLOW, None, "Notes")
                .bullets([
                    "Supported filesystems: NTFS, ext4/ext3/ext2",
                    "Back up important data before resizing",
                ])
                .build(),
            PartitioningMethodOption::Manual => PreviewBuilder::new()
                .header(NerdFont::HardDrive, "Manual Partitioning")
                .subtext("Use cfdisk to create your own partition layout.")
                .blank()
                .line(colors::TEAL, None, "You will set")
                .bullets([
                    "Root partition",
                    "Boot or EFI partition",
                    "Optional swap partition",
                ])
                .blank()
                .line(colors::TEAL, None, "Best for")
                .bullets(["Custom layouts", "Advanced users"])
                .build(),
        }
    }
}

impl FzfSelectable for PartitioningMethodOption {
    fn fzf_display_text(&self) -> String {
        self.label().to_string()
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview()
    }

    fn fzf_key(&self) -> String {
        self.label().to_string()
    }
}

#[async_trait::async_trait]
impl WizardStep for PartitioningMethodQuestion {
    fn id(&self) -> StepId {
        StepId::PartitioningMethod
    }

    fn description(&self) -> Option<&str> {
        Some("Choose how to partition the disk")
    }

    fn depends_on(&self) -> &[StepId] {
        &[StepId::Disk]
    }

    async fn run(&self, context: &InstallContext) -> Result<StepOutcome> {
        let mut options = vec![
            PartitioningMethodOption::Automatic,
            PartitioningMethodOption::Manual,
        ];

        // Check for dual boot possibility using shared feasibility logic
        if let Some(disk_path) = context.get_answer(&StepId::Disk) {
            // disk_path is now just the device path (e.g., "/dev/sda")
            let disk_path_owned = disk_path.to_string();
            let feasibility_result = tokio::task::spawn_blocking(
                move || -> anyhow::Result<crate::arch::dualboot::DualBootFeasibility> {
                    let disks = crate::arch::dualboot::detect_disks()?;
                    if let Some(disk_info) = disks.iter().find(|d| d.device == disk_path_owned) {
                        Ok(disk_info.check_disk_dualboot_feasibility())
                    } else {
                        Ok(crate::arch::dualboot::DualBootFeasibility {
                            feasible: false,
                            feasible_partitions: vec![],
                            reason: Some("Selected disk not found".to_string()),
                        })
                    }
                },
            )
            .await;

            if let Ok(Ok(feasibility)) = feasibility_result
                && feasibility.feasible
            {
                options.insert(1, PartitioningMethodOption::DualBoot);
            }
        }

        let result = FzfWrapper::builder()
            .header(HeaderBuilder::new(NerdFont::HardDrive, "Select Partitioning Method").build())
            .select_one(options)?;

        Ok(StepOutcome::from_dialog(result, |option| {
            option.label().to_string()
        }))
    }
}

pub struct RunCfdiskStep;

#[derive(Clone)]
enum EmptyLayoutAction {
    ReopenCfdisk,
    ChangePartitioningMethod,
    PauseInstaller,
}

impl FzfSelectable for EmptyLayoutAction {
    fn fzf_display_text(&self) -> String {
        match self {
            Self::ReopenCfdisk => format!("{} Reopen cfdisk", NerdFont::HardDrive),
            Self::ChangePartitioningMethod => {
                format!("{} Change partitioning method", NerdFont::ArrowLeft)
            }
            Self::PauseInstaller => format!("{} Pause installer", NerdFont::Pause),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            Self::ReopenCfdisk => FzfPreview::Text(
                "Open cfdisk again and create the required partitions.".to_string(),
            ),
            Self::ChangePartitioningMethod => {
                FzfPreview::Text("Return directly to partitioning-method selection.".to_string())
            }
            Self::PauseInstaller => FzfPreview::Text("Open the installer pause menu.".to_string()),
        }
    }
}

#[async_trait::async_trait]
impl WizardStep for RunCfdiskStep {
    fn id(&self) -> StepId {
        StepId::RunCfdisk
    }

    fn description(&self) -> Option<&str> {
        Some("Create partitions manually with cfdisk")
    }

    fn should_ask(&self, context: &InstallContext) -> bool {
        context
            .get_answer(&StepId::PartitioningMethod)
            .map(|s| s.contains("Manual"))
            .unwrap_or(false)
    }

    fn depends_on(&self) -> &[StepId] {
        &[StepId::Disk, StepId::PartitioningMethod]
    }

    fn completion_is_current(&self, context: &InstallContext) -> bool {
        context
            .get_answer(&StepId::Disk)
            .and_then(|disk| super::partition::list_partitions(disk).ok())
            .is_some_and(|partitions| !partitions.is_empty())
    }

    async fn run(&self, context: &InstallContext) -> Result<StepOutcome> {
        // disk is now just the device path (e.g., "/dev/sda")
        let disk_path = context
            .get_answer(&StepId::Disk)
            .context("No disk selected")?;

        // Check for cfdisk
        if !crate::common::deps::CFDISK.is_installed() {
            // Prompt to install cfdisk
            crate::common::package::ensure_all(&[&crate::common::deps::CFDISK])
                .context("cfdisk is required for manual partitioning but could not be installed")?;
        }

        loop {
            println!("Starting cfdisk on {}...", disk_path);
            println!("Please create your partitions and write changes before exiting.");

            if !crate::common::terminal::run_tui_program("cfdisk", &[disk_path]).await? {
                return Ok(StepOutcome::Pause);
            }

            if which::which("udevadm").is_ok() {
                let status = std::process::Command::new("udevadm")
                    .arg("settle")
                    .status()
                    .context("Failed to wait for partition device state")?;
                anyhow::ensure!(status.success(), "udevadm settle failed after cfdisk");
            }

            if !super::partition::list_partitions(disk_path)?.is_empty() {
                return Ok(StepOutcome::Completed);
            }

            let result = FzfWrapper::builder()
                .header(
                    HeaderBuilder::new(
                        NerdFont::Warning,
                        format!("No partitions found on {disk_path}"),
                    )
                    .build(),
                )
                .presentation(MenuPresentation::Padded)
                .select_one(vec![
                    EmptyLayoutAction::ReopenCfdisk,
                    EmptyLayoutAction::ChangePartitioningMethod,
                    EmptyLayoutAction::PauseInstaller,
                ])?;

            match result {
                crate::menu_utils::DialogOutcome::Submitted(EmptyLayoutAction::ReopenCfdisk) => {
                    continue;
                }
                crate::menu_utils::DialogOutcome::Submitted(
                    EmptyLayoutAction::ChangePartitioningMethod,
                ) => {
                    return Ok(StepOutcome::revisit(
                        StepId::PartitioningMethod,
                        "Choose a different partitioning method.",
                    ));
                }
                crate::menu_utils::DialogOutcome::Submitted(EmptyLayoutAction::PauseInstaller)
                | crate::menu_utils::DialogOutcome::Cancelled => {
                    return Ok(StepOutcome::Pause);
                }
            }
        }
    }
}
