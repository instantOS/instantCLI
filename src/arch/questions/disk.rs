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

        let result = FzfWrapper::builder()
            .header(format!("{} Select Installation Disk", NerdFont::HardDrive))
            .select(disks)?;

        match result {
            crate::menu_utils::FzfResult::Selected(disk) => Ok(QuestionResult::Answer(disk)),
            crate::menu_utils::FzfResult::Cancelled => Ok(QuestionResult::Cancelled),
            _ => Ok(QuestionResult::Cancelled),
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

        // Check if disk is mounted
        if let Ok(true) = crate::arch::disks::is_disk_mounted(device_name) {
            return Err(format!(
                "The selected disk ({}) contains mounted partitions.\n\
                Please unmount all partitions on this disk before proceeding.",
                device_name
            ));
        }

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

        // Check for dual boot possibility
        if let Some(disk_str) = context.get_answer(&QuestionId::Disk) {
            let disk_path = disk_str.split('(').next().unwrap_or(disk_str).trim();

            let disk_path_owned = disk_path.to_string();
            let disks_result =
                tokio::task::spawn_blocking(move || crate::arch::dualboot::detect_disks()).await?;

            if let Ok(disks) = disks_result {
                if let Some(disk_info) = disks.iter().find(|d| d.device == disk_path_owned) {
                    // Check if any partition is shrinkable
                    // We primarily care about partitions that are not the ESP and can be shrunk
                    let shrinkable = disk_info.partitions.iter().any(|p| {
                        !p.is_efi
                            && p.resize_info
                                .as_ref()
                                .map(|r| r.can_shrink)
                                .unwrap_or(false)
                    });

                    if shrinkable {
                        options.insert(1, "Dual Boot (Experimental)".to_string());
                    }
                }
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

        // Register signal handler BEFORE spawning child to catch the signal
        let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;

        // Use spawn_blocking to run cfdisk in a sync context
        // This avoids async runtime interference with terminal control
        let disk_path = disk_path.to_string();
        let child_task = tokio::task::spawn_blocking(move || {
            use std::fs::OpenOptions;
            use std::process::{Command, Stdio};

            // Open /dev/tty explicitly to ensure we have a valid terminal
            // This fixes issues where sudo/tokio might interfere with stdin/stdout inheritance
            let tty = OpenOptions::new()
                .read(true)
                .write(true)
                .open("/dev/tty")
                .expect("Failed to open /dev/tty");

            // We need separate handles for each stream
            let tty_in = tty.try_clone().expect("Failed to clone tty handle");
            let tty_out = tty.try_clone().expect("Failed to clone tty handle");
            let tty_err = tty.try_clone().expect("Failed to clone tty handle");

            let mut child = Command::new("cfdisk")
                .arg(disk_path)
                .stdin(Stdio::from(tty_in))
                .stdout(Stdio::from(tty_out))
                .stderr(Stdio::from(tty_err))
                .spawn()
                .expect("Failed to spawn cfdisk");

            // Just wait for cfdisk to complete
            child.wait()
        });

        tokio::select! {
            res = child_task => {
                // Task completed (cfdisk exited normally)
                match res {
                    Ok(Ok(status)) => {
                        if status.success() {
                            Ok(QuestionResult::Answer("done".to_string()))
                        } else {
                            Ok(QuestionResult::Cancelled)
                        }
                    }
                    Ok(Err(e)) => Err(anyhow::anyhow!("Failed to wait for cfdisk: {}", e)),
                    Err(e) => Err(anyhow::anyhow!("Task join error: {}", e)),
                }
            }
            _ = sigint.recv() => {
                // User pressed Ctrl+C
                println!("\ncfdisk cancelled by user.");
                Ok(QuestionResult::Cancelled)
            }
        }
    }
}
