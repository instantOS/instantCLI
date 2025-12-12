use crate::arch::engine::{DataKey, InstallContext, Question, QuestionId, QuestionResult};
use crate::menu_utils::FzfWrapper;
use crate::ui::nerd_font::NerdFont;
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
            let _ = FzfWrapper::message_dialog(&message);
            Ok(false)
        }
    }
}

pub struct DiskQuestion;

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

        if disks.is_empty() {
            return Ok(QuestionResult::Cancelled);
        }

        loop {
            let result = FzfWrapper::builder()
                .header(format!("{} Select Installation Disk", NerdFont::HardDrive))
                .select(disks.clone())?;

            let disk = match result {
                crate::menu_utils::FzfResult::Selected(d) => d,
                _ => return Ok(QuestionResult::Cancelled),
            };

            // disk is now a DiskEntry, get just the path
            if try_prepare_disk(&disk.path)? {
                // Store just the path, not the formatted display string
                return Ok(QuestionResult::Answer(disk.path));
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

    fn fatal_error_message(&self, context: &InstallContext) -> Option<String> {
        let disks = context
            .get::<crate::arch::disks::DisksKey>()
            .unwrap_or_default();

        if disks.is_empty() {
            Some(
                "No disks were detected on this system.\n\n\
                Possible causes:\n\
                • The system has no disks installed\n\
                • You are not running with root/sudo privileges\n\
                • The disk driver is not loaded\n\n\
                Please check your hardware and ensure you are running as root."
                    .to_string(),
            )
        } else {
            None
        }
    }
}

pub struct PartitioningMethodQuestion;

#[async_trait::async_trait]
impl Question for PartitioningMethodQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::PartitioningMethod
    }

    async fn ask(&self, context: &InstallContext) -> Result<QuestionResult> {
        let mut options = vec![
            "Automatic (Erase Disk)".to_string(),
            "Manual (cfdisk)".to_string(),
        ];

        // Check for dual boot possibility using shared feasibility logic
        if let Some(disk_path) = context.get_answer(&QuestionId::Disk) {
            // disk_path is now just the device path (e.g., "/dev/sda")
            let disk_path_owned = disk_path.to_string();
            let feasibility_result = tokio::task::spawn_blocking(
                move || -> anyhow::Result<crate::arch::dualboot::DualBootFeasibility> {
                    let disks = crate::arch::dualboot::detect_disks()?;
                    if let Some(disk_info) = disks.iter().find(|d| d.device == disk_path_owned) {
                        Ok(crate::arch::dualboot::check_disk_dualboot_feasibility(
                            disk_info,
                        ))
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
                options.insert(1, "Dual Boot (Experimental)".to_string());
            }
        }

        let result = FzfWrapper::builder()
            .header(format!(
                "{} Select Partitioning Method",
                NerdFont::HardDrive
            ))
            .select(options)?;

        match result {
            crate::menu_utils::FzfResult::Selected(s) => Ok(QuestionResult::Answer(s)),
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
        if !crate::common::requirements::CFDISK_PACKAGE.is_installed() {
            // Try to install cfdisk if missing
            if let Err(e) = crate::common::requirements::CFDISK_PACKAGE.ensure() {
                return Err(anyhow::anyhow!(
                    "cfdisk is required for manual partitioning but could not be installed: {}",
                    e
                ));
            }
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
