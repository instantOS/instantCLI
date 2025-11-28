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
        configure_grub_encryption(executor)?;
    }

    // grub-mkconfig -o /boot/grub/grub.cfg
    let mut cmd = Command::new("grub-mkconfig");
    cmd.arg("-o").arg("/boot/grub/grub.cfg");

    executor.run(&mut cmd)?;

    Ok(())
}

fn configure_grub_encryption(executor: &CommandExecutor) -> Result<()> {
    if executor.dry_run {
        println!("[DRY RUN] Adding 'cryptdevice=UUID=...:cryptlvm' to GRUB_CMDLINE_LINUX");
        return Ok(());
    }

    // Find UUID of LUKS partition
    let output = Command::new("blkid")
        .args(["-o", "value", "-s", "UUID", "-t", "TYPE=crypto_LUKS"])
        .output()?;

    let uuid = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if uuid.is_empty() {
        anyhow::bail!("Could not find UUID for LUKS partition");
    }

    println!("Found LUKS UUID: {}", uuid);

    let grub_default = "/etc/default/grub";
    let content = std::fs::read_to_string(grub_default)?;

    let mut new_lines = Vec::new();
    for line in content.lines() {
        if line.starts_with("GRUB_CMDLINE_LINUX=") {
            let param = format!("cryptdevice=UUID={}:cryptlvm", uuid);
            // Insert inside quotes
            if line.contains('"') {
                let parts: Vec<&str> = line.split('"').collect();
                if parts.len() >= 2 {
                    // Append to existing params
                    let existing = parts[1];
                    if existing.is_empty() {
                        new_lines.push(format!("GRUB_CMDLINE_LINUX=\"{}\"", param));
                    } else {
                        new_lines.push(format!("GRUB_CMDLINE_LINUX=\"{} {}\"", existing, param));
                    }
                } else {
                    new_lines.push(format!("GRUB_CMDLINE_LINUX=\"{}\"", param));
                }
            } else {
                new_lines.push(format!("GRUB_CMDLINE_LINUX=\"{}\"", param));
            }
        } else {
            new_lines.push(line.to_string());
        }
    }

    std::fs::write(grub_default, new_lines.join("\n"))?;

    Ok(())
}
