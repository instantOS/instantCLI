use crate::arch::engine::{DataKey, InstallContext, Question, QuestionId, QuestionResult};
use crate::menu_utils::FzfWrapper;
use crate::ui::nerd_font::NerdFont;
use anyhow::{Context, Result};

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

            match result {
                crate::menu_utils::FzfResult::Selected(disk) => {
                    // Extract device path
                    let device_name = disk.split('(').next().unwrap_or(&disk).trim();

                    // Check for mounted partitions and offer to unmount
                    if let Ok(mounted) = crate::arch::disks::get_mounted_partitions(device_name) {
                        if !mounted.is_empty() {
                            println!(
                                "\n{} The disk {} has mounted partitions:",
                                NerdFont::Warning,
                                device_name
                            );
                            for part in &mounted {
                                println!("  • {}", part);
                            }

                            match FzfWrapper::confirm("Unmount these partitions automatically?") {
                                Ok(crate::menu_utils::ConfirmResult::Yes) => {
                                    match crate::arch::disks::unmount_disk(device_name) {
                                        Ok(unmounted) => {
                                            println!(
                                                "{} Successfully unmounted {} partition(s)",
                                                NerdFont::Check,
                                                unmounted.len()
                                            );
                                        }
                                        Err(e) => {
                                            println!(
                                                "{} Failed to unmount: {}",
                                                NerdFont::Cross,
                                                e
                                            );
                                            println!("Please unmount manually and try again.");
                                            continue; // Let user select again
                                        }
                                    }
                                }
                                _ => {
                                    println!("Please unmount the partitions manually and try again.");
                                    continue; // Let user select again
                                }
                            }
                        }
                    }

                    return Ok(QuestionResult::Answer(disk));
                }
                crate::menu_utils::FzfResult::Cancelled => return Ok(QuestionResult::Cancelled),
                _ => return Ok(QuestionResult::Cancelled),
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

        // Extract device name from the selection (e.g., "/dev/sda (500 GiB)" -> "/dev/sda")
        let device_name = answer.split('(').next().unwrap_or(answer).trim();

        // Get the root filesystem device to check against
        if let Ok(Some(root_device)) = crate::arch::disks::get_root_device() {
            // Check if the selected device is exactly the root filesystem device
            if device_name == root_device {
                return Err(format!(
                    "Cannot select the current root filesystem device ({}) for installation.\n\
                    This device contains the currently running system and would cause data loss.\n\
                    Please select a different disk.",
                    root_device
                ));
            }
        }

        // Check if this disk is the current boot disk (physical disk containing root)
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

        // Note: mounted partition check is now handled interactively in ask()
        // with an offer to automatically unmount

        // Check if disk is used as swap
        if let Ok(true) = crate::arch::disks::is_disk_swap(device_name) {
            return Err(format!(
                "The selected disk ({}) is currently being used as swap.\n\
                Please swapoff this disk before proceeding.",
                device_name
            ));
        }

        Ok(())
    }

    fn data_providers(&self) -> Vec<Box<dyn crate::arch::engine::AsyncDataProvider>> {
        vec![Box::new(crate::arch::disks::DiskProvider)]
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
        if let Some(disk_str) = context.get_answer(&QuestionId::Disk) {
            let disk_path = disk_str.split('(').next().unwrap_or(disk_str).trim();

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
        let disk = context
            .get_answer(&QuestionId::Disk)
            .context("No disk selected")?;

        let disk_path = disk.split('(').next().unwrap_or(disk).trim();

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
