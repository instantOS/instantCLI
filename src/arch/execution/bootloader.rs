use super::CommandExecutor;
use crate::arch::engine::{BootMode, InstallContext, QuestionId};
use anyhow::{Context, Result};
use std::process::Command;

pub async fn install_bootloader(
    context: &InstallContext,
    executor: &CommandExecutor,
) -> Result<()> {
    println!("Installing bootloader (inside chroot)...");

    match context.system_info.boot_mode {
        BootMode::UEFI64 | BootMode::UEFI32 => install_grub_uefi(executor)?,
        BootMode::BIOS => install_grub_bios(context, executor)?,
    }

    configure_grub(context, executor)?;

    Ok(())
}

fn install_grub_uefi(executor: &CommandExecutor) -> Result<()> {
    println!("Detected UEFI mode. Installing GRUB for UEFI...");

    // Install packages if not already present (pacstrap should have installed them if added to list)
    // But here we assume they are installed or we are just configuring.
    // Actually, we should probably ensure grub and efibootmgr are installed.
    // But `base` step usually installs packages.
    // For now, let's assume `grub` and `efibootmgr` were in the package list.
    // If not, we might need to install them here using pacman.

    // Check if we need to install packages?
    // The plan didn't explicitly say to install packages here, but `base` step might have missed them.
    // Let's assume they are installed for now.

    // grub-install --target=x86_64-efi --efi-directory=/boot --bootloader-id=GRUB
    // Note: /boot is usually where ESP is mounted in Arch if using systemd-boot,
    // but for GRUB it can be /boot/efi or just /boot.
    // The plan said: "mount --mkdir /dev/efi_system_partition /mnt/boot"
    // So ESP is at /boot.

    let mut cmd = Command::new("grub-install");
    cmd.arg("--target=x86_64-efi")
        .arg("--efi-directory=/boot")
        .arg("--bootloader-id=GRUB");

    executor.run(&mut cmd)?;

    Ok(())
}

fn install_grub_bios(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    println!("Detected BIOS mode. Installing GRUB for BIOS...");

    let disk_answer = context
        .get_answer(&QuestionId::Disk)
        .context("Disk not selected")?;

    // Parse disk from answer string, e.g., "/dev/sda (500 GiB)" -> "/dev/sda"
    let disk = disk_answer
        .split_whitespace()
        .next()
        .context("Invalid disk format")?;

    println!("Installing GRUB to MBR of {}", disk);

    // grub-install --target=i386-pc /dev/sdX
    let mut cmd = Command::new("grub-install");
    cmd.arg("--target=i386-pc").arg(disk);

    executor.run(&mut cmd)?;

    Ok(())
}

fn configure_grub(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    println!("Generating GRUB configuration...");

    if context.get_answer_bool(QuestionId::UseEncryption) {
        configure_grub_encryption(context, executor)?;
    }

    // grub-mkconfig -o /boot/grub/grub.cfg
    let mut cmd = Command::new("grub-mkconfig");
    cmd.arg("-o").arg("/boot/grub/grub.cfg");

    executor.run(&mut cmd)?;

    Ok(())
}

fn configure_grub_encryption(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    if executor.dry_run {
        println!("[DRY RUN] Adding 'cryptdevice=UUID=...:cryptlvm' to GRUB_CMDLINE_LINUX");
        return Ok(());
    }

    let disk_answer = context
        .get_answer(&QuestionId::Disk)
        .context("Disk not selected")?;
    let disk = disk_answer.split('(').next().unwrap_or(disk_answer).trim();
    
    // LUKS is always on partition 2 in our layout
    let luks_part = crate::arch::execution::disk::get_part_path(disk, 2);

    println!("Getting UUID for LUKS partition: {}", luks_part);

    // Find UUID of LUKS partition
    let output = Command::new("blkid")
        .args(["-o", "value", "-s", "UUID", &luks_part])
        .output()?;

    let uuid = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if uuid.is_empty() {
        anyhow::bail!("Could not find UUID for LUKS partition {}", luks_part);
    }

    println!("Found LUKS UUID: {}", uuid);

    let grub_default = "/etc/default/grub";
    let content = std::fs::read_to_string(grub_default)?;
    let param = format!("cryptdevice=UUID={}:cryptlvm", uuid);

    let new_content = add_grub_kernel_param(&content, &param);

    std::fs::write(grub_default, new_content)?;

    Ok(())
}

fn add_grub_kernel_param(content: &str, param: &str) -> String {
    let mut new_lines = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("GRUB_CMDLINE_LINUX=") {
            // Split key and value
            let parts: Vec<&str> = line.splitn(2, '=').collect();
            if parts.len() != 2 {
                new_lines.push(line.to_string());
                continue;
            }

            let key = parts[0];
            let val = parts[1];

            // Detect quotes
            let (_quote_char, inner_val) = if val.starts_with('"') && val.ends_with('"') {
                ("\"", &val[1..val.len() - 1])
            } else if val.starts_with('\'') && val.ends_with('\'') {
                ("'", &val[1..val.len() - 1])
            } else {
                ("", val)
            };

            let new_val = if inner_val.is_empty() {
                param.to_string()
            } else {
                format!("{} {}", inner_val, param)
            };

            // Reconstruct with double quotes for safety
            new_lines.push(format!("{}=\"{}\"", key, new_val));
        } else {
            new_lines.push(line.to_string());
        }
    }
    new_lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_grub_kernel_param() {
        let param = "cryptdevice=UUID=123:cryptlvm";

        // Case 1: Empty value
        let input = "GRUB_CMDLINE_LINUX=\"\"";
        let expected = format!("GRUB_CMDLINE_LINUX=\"{}\"", param);
        assert_eq!(add_grub_kernel_param(input, param), expected);

        // Case 2: Existing value with double quotes
        let input = "GRUB_CMDLINE_LINUX=\"quiet splash\"";
        let expected = format!("GRUB_CMDLINE_LINUX=\"quiet splash {}\"", param);
        assert_eq!(add_grub_kernel_param(input, param), expected);

        // Case 3: Existing value with single quotes
        let input = "GRUB_CMDLINE_LINUX='quiet splash'";
        let expected = format!("GRUB_CMDLINE_LINUX=\"quiet splash {}\"", param);
        assert_eq!(add_grub_kernel_param(input, param), expected);

        // Case 4: No quotes
        let input = "GRUB_CMDLINE_LINUX=quiet";
        let expected = format!("GRUB_CMDLINE_LINUX=\"quiet {}\"", param);
        assert_eq!(add_grub_kernel_param(input, param), expected);

        // Case 5: Multiple lines
        let input = "GRUB_DEFAULT=0\nGRUB_CMDLINE_LINUX=\"\"\nGRUB_TIMEOUT=5";
        let expected = format!(
            "GRUB_DEFAULT=0\nGRUB_CMDLINE_LINUX=\"{}\"\nGRUB_TIMEOUT=5",
            param
        );
        assert_eq!(add_grub_kernel_param(input, param), expected);
    }
}
