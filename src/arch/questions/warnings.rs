use crate::arch::engine::{BootMode, InstallContext, Question, QuestionId, QuestionResult};
use crate::menu_utils::FzfWrapper;
use crate::ui::nerd_font::NerdFont;
use anyhow::{Context, Result};

pub struct LowRamWarning;

#[async_trait::async_trait]
impl Question for LowRamWarning {
    fn id(&self) -> QuestionId {
        QuestionId::LowRamWarning
    }

    fn should_ask(&self, context: &InstallContext) -> bool {
        // Show warning if RAM is detected and less than 1GB
        context.system_info.total_ram_gb.is_some_and(|ram| ram < 1)
    }

    async fn ask(&self, context: &InstallContext) -> Result<QuestionResult> {
        let ram_gb = context.system_info.total_ram_gb.unwrap_or(0);
        FzfWrapper::message(&format!(
            "{} Low Memory Warning\n\n\
             System has {} GB of RAM (less than 1 GB).\n\
             Installation may be slow or fail.\n\n\
             Installation will proceed, but consider upgrading your RAM for a better experience.",
            NerdFont::Warning,
            ram_gb
        ))?;
        Ok(QuestionResult::Answer("acknowledged".to_string()))
    }
}

pub struct VirtualBoxWarning;

#[async_trait::async_trait]
impl Question for VirtualBoxWarning {
    fn id(&self) -> QuestionId {
        QuestionId::VirtualBoxWarning
    }

    fn should_ask(&self, context: &InstallContext) -> bool {
        if let Some(vm_type) = &context.system_info.vm_type {
            let vm = vm_type.to_lowercase();
            vm.contains("oracle") || vm.contains("virtualbox")
        } else {
            false
        }
    }

    async fn ask(&self, _context: &InstallContext) -> Result<QuestionResult> {
        FzfWrapper::message(&format!(
            "{} VirtualBox Detected!\n\n\
             Wayland does not work properly in VirtualBox.\n\
             Please use X11 or another hypervisor for the best experience.",
            NerdFont::Warning
        ))?;
        Ok(QuestionResult::Answer("acknowledged".to_string()))
    }
}

pub struct WeakPasswordWarning;

#[async_trait::async_trait]
impl Question for WeakPasswordWarning {
    fn id(&self) -> QuestionId {
        QuestionId::WeakPasswordWarning
    }

    fn should_ask(&self, context: &InstallContext) -> bool {
        if !context.get_answer_bool(QuestionId::UseEncryption) {
            return false;
        }
        if let Some(pass) = context.get_answer(&QuestionId::EncryptionPassword) {
            pass.len() < 4
        } else {
            false
        }
    }

    async fn ask(&self, _context: &InstallContext) -> Result<QuestionResult> {
        FzfWrapper::message(&format!(
            "{} Weak Password Warning\n\n\
             The encryption password is shorter than 4 characters.\n\
             This is considered insecure.",
            NerdFont::Warning
        ))?;
        Ok(QuestionResult::Answer("acknowledged".to_string()))
    }
}

pub struct DualBootEspWarning;

#[async_trait::async_trait]
impl Question for DualBootEspWarning {
    fn id(&self) -> QuestionId {
        QuestionId::DualBootEspWarning
    }

    fn should_ask(&self, context: &InstallContext) -> bool {
        let is_dualboot = context
            .get_answer(&QuestionId::PartitioningMethod)
            .map(|s| s.contains("Dual Boot"))
            .unwrap_or(false);

        if !is_dualboot {
            return false;
        }

        if !matches!(
            context.system_info.boot_mode,
            BootMode::UEFI32 | BootMode::UEFI64
        ) {
            return false;
        }

        let disk_path = match context.get_answer(&QuestionId::Disk) {
            Some(path) => path,
            None => return false,
        };

        let disks = context
            .get::<crate::arch::dualboot::DisksKey>()
            .unwrap_or_default();

        if disks.is_empty() {
            return false;
        }

        let Some(disk_info) = disks.iter().find(|d| d.device == *disk_path) else {
            return false;
        };

        let esp_partitions: Vec<_> = disk_info.partitions.iter().filter(|p| p.is_efi).collect();

        if esp_partitions.is_empty() {
            return true;
        }

        esp_partitions
            .iter()
            .all(|p| p.size_bytes < crate::arch::dualboot::types::MIN_ESP_SIZE)
    }

    async fn ask(&self, context: &InstallContext) -> Result<QuestionResult> {
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

        let esp_partitions: Vec<_> = disk_info.partitions.iter().filter(|p| p.is_efi).collect();

        let mut message = format!("{} EFI System Partition Warning\n\n", NerdFont::Warning);

        if esp_partitions.is_empty() {
            message.push_str(
                "No EFI System Partition was detected on the selected disk.\n\
                 The installer will create a new 260MB ESP if space is available.\n\
                 If the disk is full, resize partitions or use manual partitioning.",
            );
        } else {
            message.push_str("Existing EFI System Partitions are smaller than 260MB:\n");
            for esp in esp_partitions {
                message.push_str(&format!(
                    "  {} ({})\n",
                    esp.device,
                    crate::arch::dualboot::format_size(esp.size_bytes)
                ));
            }
            message.push_str(
                "\nThe installer will create a new 260MB ESP if space is available.\n\
                 Keep the existing ESP intact to preserve other operating systems.",
            );
        }

        FzfWrapper::message(&message)?;
        Ok(QuestionResult::Answer("acknowledged".to_string()))
    }
}
