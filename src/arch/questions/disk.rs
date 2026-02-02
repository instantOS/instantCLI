use crate::arch::dualboot::feasibility::check_disk_dualboot_feasibility;
use crate::arch::engine::{DataKey, InstallContext, Question, QuestionId, QuestionResult};
use crate::menu_utils::{FzfPreview, FzfResult, FzfSelectable, FzfWrapper};
use crate::ui::catppuccin::colors;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;
use anyhow::{Context, Result};

/// Attempts to prepare a disk for installation.
/// Returns Ok(true) if disk is ready, Ok(false) if user declined preparation.
fn try_prepare_disk(device_name: &str) -> Result<bool> {
    let mounted = crate::arch::disks::get_mounted_partitions(device_name).unwrap_or_default();
    let swap = crate::arch::disks::get_swap_partitions(device_name).unwrap_or_default();

    if mounted.is_empty() && swap.is_empty() {
        return Ok(true); // Disk is ready
    }

    // Show what needs to be unmounted/disabled
    println!(
        "\n{} The disk {} is currently in use:",
        NerdFont::Warning,
        device_name
    );
    for part in &mounted {
        println!("  • {} (mounted)", part);
    }
    for part in &swap {
        println!("  • {} (swap)", part);
    }

    // Ask for confirmation
    let confirmed = matches!(
        FzfWrapper::confirm("Unmount partitions and disable swap automatically?"),
        Ok(crate::menu_utils::ConfirmResult::Yes)
    );

    if !confirmed {
        println!("Please prepare the disk manually and try again.");
        return Ok(false);
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
            Ok(true)
        }
        Err(e) => {
            let message = format!(
                "Failed to prepare disk {}:\n{}\n\nPlease prepare the disk manually and try again.",
                device_name, e
            );
            // Show the error via the menu UI so the user cannot miss it
            let _ = FzfWrapper::message(&message);
            Ok(false)
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
            let mut builder = FzfWrapper::builder()
                .prompt("Custom disk path")
                .ghost("/dev/nvme0n1")
                .input();

            if let Some(previous) = last_custom_path.as_ref() {
                builder = builder.query(previous.clone());
            }

            let input = builder.input_result()?;
            let path = match input {
                FzfResult::Selected(value) => value.trim().to_string(),
                FzfResult::Cancelled => return Ok(None),
                _ => return Ok(None),
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

            if try_prepare_disk(&path)? {
                return Ok(Some(path));
            }
        }
    }
}

#[async_trait::async_trait]
impl Question for DiskQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::Disk
    }

    fn required_data_keys(&self) -> Vec<String> {
        vec![crate::arch::disks::DisksKey::KEY.to_string()]
    }

    async fn ask(&self, context: &InstallContext) -> Result<QuestionResult> {
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
                .header(format!("{} Select Installation Disk", NerdFont::HardDrive))
                .select(selections.clone())?;

            let selection = match result {
                crate::menu_utils::FzfResult::Selected(d) => d,
                _ => return Ok(QuestionResult::Cancelled),
            };

            match selection {
                DiskSelection::Detected(disk) => {
                    if try_prepare_disk(&disk.path)? {
                        // Store just the path, not the formatted display string
                        return Ok(QuestionResult::Answer(disk.path));
                    }
                }
                DiskSelection::CustomPath => {
                    if let Some(path) =
                        self.prompt_custom_disk_path(context, &mut last_custom_path)?
                    {
                        return Ok(QuestionResult::Answer(path));
                    }
                }
            }
            // User declined or preparation failed - loop back to disk selection
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
impl Question for PartitioningMethodQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::PartitioningMethod
    }

    async fn ask(&self, context: &InstallContext) -> Result<QuestionResult> {
        let mut options = vec![
            PartitioningMethodOption::Automatic,
            PartitioningMethodOption::Manual,
        ];

        // Check for dual boot possibility using shared feasibility logic
        if let Some(disk_path) = context.get_answer(&QuestionId::Disk) {
            // disk_path is now just the device path (e.g., "/dev/sda")
            let disk_path_owned = disk_path.to_string();
            let feasibility_result = tokio::task::spawn_blocking(
                move || -> anyhow::Result<crate::arch::dualboot::DualBootFeasibility> {
                    let disks = crate::arch::dualboot::detect_disks()?;
                    if let Some(disk_info) = disks.iter().find(|d| d.device == disk_path_owned) {
                        Ok(check_disk_dualboot_feasibility(disk_info))
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
            .header(format!(
                "{} Select Partitioning Method",
                NerdFont::HardDrive
            ))
            .select(options)?;

        match result {
            crate::menu_utils::FzfResult::Selected(option) => {
                Ok(QuestionResult::Answer(option.label().to_string()))
            }
            crate::menu_utils::FzfResult::Cancelled => Ok(QuestionResult::Cancelled),
            _ => Ok(QuestionResult::Cancelled),
        }
    }
}

pub struct RunCfdiskQuestion;

#[async_trait::async_trait]
impl Question for RunCfdiskQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::RunCfdisk
    }

    fn should_ask(&self, context: &InstallContext) -> bool {
        context
            .get_answer(&QuestionId::PartitioningMethod)
            .map(|s| s.contains("Manual"))
            .unwrap_or(false)
    }

    async fn ask(&self, context: &InstallContext) -> Result<QuestionResult> {
        // disk is now just the device path (e.g., "/dev/sda")
        let disk_path = context
            .get_answer(&QuestionId::Disk)
            .context("No disk selected")?;

        // Check for cfdisk
        if !crate::common::deps::CFDISK.is_installed() {
            // Prompt to install cfdisk
            crate::common::package::ensure_all(&[&crate::common::deps::CFDISK])
                .context("cfdisk is required for manual partitioning but could not be installed")?;
        }

        println!("Starting cfdisk on {}...", disk_path);
        println!("Please create your partitions and save changes before exiting.");

        // Use the shared TUI program runner that handles tokio/terminal issues
        match crate::common::terminal::run_tui_program("cfdisk", &[disk_path]).await {
            Ok(true) => Ok(QuestionResult::Answer("done".to_string())),
            Ok(false) => {
                println!("\ncfdisk cancelled by user.");
                Ok(QuestionResult::Cancelled)
            }
            Err(e) => Err(e),
        }
    }
}
