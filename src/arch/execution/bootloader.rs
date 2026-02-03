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
        BootMode::UEFI64 | BootMode::UEFI32 => install_grub_uefi(context, executor)?,
        BootMode::BIOS => install_grub_bios(context, executor)?,
    }

    configure_grub(context, executor)?;

    Ok(())
}

/// Packages needed for bootloader setup (installed in a single batch elsewhere)
pub fn bootloader_package_list(context: &InstallContext) -> Vec<String> {
    let mut packages = vec!["grub".to_string(), "os-prober".to_string()];

    if matches!(
        context.system_info.boot_mode,
        BootMode::UEFI64 | BootMode::UEFI32
    ) {
        packages.push("efibootmgr".to_string());
    }

    packages
}

fn install_grub_uefi(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    println!("Detected UEFI mode. Installing GRUB for UEFI...");

    // Determine the appropriate target based on UEFI mode
    let target = match context.system_info.boot_mode {
        BootMode::UEFI64 => "x86_64-efi",
        BootMode::UEFI32 => "i386-efi",
        _ => anyhow::bail!("Invalid boot mode for UEFI installation"),
    };

    println!("Installing GRUB with target: {}", target);

    // Install GRUB for UEFI
    // Use /boot/efi when present (dual-boot reuse) otherwise /boot (fresh installs)
    let efi_dir = if std::path::Path::new("/boot/efi").exists() {
        "/boot/efi"
    } else {
        "/boot"
    };

    let mut cmd = Command::new("grub-install");
    cmd.arg(format!("--target={}", target))
        .arg(format!("--efi-directory={}", efi_dir))
        .arg("--bootloader-id=GRUB")
        .arg("--recheck"); // Ensure GRUB is properly installed

    executor.run(&mut cmd)?;

    Ok(())
}

fn install_grub_bios(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    println!("Detected BIOS mode. Installing GRUB for BIOS...");

    // disk is now just the device path (e.g., "/dev/sda")
    let disk = context
        .get_answer(&QuestionId::Disk)
        .context("Disk not selected")?;

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

    if context.get_answer_bool(QuestionId::UsePlymouth)
        && !context.get_answer_bool(QuestionId::MinimalMode)
    {
        configure_grub_plymouth(context, executor)?;
    }

    if !context.get_answer_bool(QuestionId::MinimalMode) {
        configure_grub_theme(context, executor)?;
    }

    // grub-mkconfig -o /boot/grub/grub.cfg
    let mut cmd = Command::new("grub-mkconfig");
    cmd.arg("-o").arg("/boot/grub/grub.cfg");

    executor.run(&mut cmd)?;

    Ok(())
}

fn configure_grub_encryption(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    if executor.dry_run {
        println!("[DRY RUN] Adding 'rd.luks.name=...=cryptlvm' to GRUB_CMDLINE_LINUX");
        println!("[DRY RUN] Setting GRUB_ENABLE_CRYPTODISK=y in /etc/default/grub");
        return Ok(());
    }

    let luks_part = luks_partition_path(context)?;
    let uuid = read_luks_uuid(&luks_part)?;
    println!("Found LUKS UUID: {}", uuid);

    let grub_default = "/etc/default/grub";
    let content = std::fs::read_to_string(grub_default)?;
    let param = build_grub_encryption_param(&uuid);
    let new_content = set_grub_cryptodisk_enabled(&add_grub_kernel_param(&content, &param));
    std::fs::write(grub_default, new_content)?;

    Ok(())
}

fn luks_partition_path(context: &InstallContext) -> Result<String> {
    let disk = context
        .get_answer(&QuestionId::Disk)
        .context("Disk not selected")?;

    Ok(crate::arch::execution::disk::get_part_path(disk, 2))
}

fn read_luks_uuid(luks_part: &str) -> Result<String> {
    println!("Getting UUID for LUKS partition: {}", luks_part);

    let output = Command::new("blkid")
        .args(["-o", "value", "-s", "UUID", luks_part])
        .output()?;

    if !output.status.success() {
        anyhow::bail!("blkid failed to get UUID for {}", luks_part);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let uuid = stdout.lines().next().unwrap_or("").trim().to_string();

    if uuid.is_empty() {
        anyhow::bail!("Could not find UUID for LUKS partition {}", luks_part);
    }

    Ok(uuid)
}

fn build_grub_encryption_param(uuid: &str) -> String {
    format!(
        "rd.luks.name={}=cryptlvm root=/dev/mapper/instantOS-root resume=/dev/mapper/instantOS-swap",
        uuid
    )
}

fn set_grub_cryptodisk_enabled(content: &str) -> String {
    if content.contains("GRUB_ENABLE_CRYPTODISK=y") {
        return content.to_string();
    }

    if content.contains("GRUB_ENABLE_CRYPTODISK=") {
        let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        for line in &mut lines {
            if line.trim().starts_with("GRUB_ENABLE_CRYPTODISK=") {
                *line = "GRUB_ENABLE_CRYPTODISK=y".to_string();
            }
        }
        return lines.join("\n");
    }

    let mut new_content = content.to_string();
    new_content.push_str("\nGRUB_ENABLE_CRYPTODISK=y\n");
    new_content
}

fn configure_grub_plymouth(_context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    if executor.dry_run {
        println!("[DRY RUN] Adding 'splash quiet' to GRUB_CMDLINE_LINUX");
        return Ok(());
    }

    let grub_default = "/etc/default/grub";
    let content = std::fs::read_to_string(grub_default)?;

    // Add splash and quiet parameters for Plymouth to GRUB_CMDLINE_LINUX_DEFAULT
    // This is where splash usually goes in Arch
    let param = "splash";
    let new_content = add_grub_param(&content, "GRUB_CMDLINE_LINUX_DEFAULT", param);

    std::fs::write(grub_default, new_content)?;

    Ok(())
}

pub fn configure_grub_theme(_context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    let grub_default = "/etc/default/grub";

    if !std::path::Path::new(grub_default).exists() {
        println!("GRUB configuration file not found, skipping theme setup.");
        return Ok(());
    }

    if executor.dry_run {
        println!("[DRY RUN] Setting GRUB_THEME in /etc/default/grub");
        return Ok(());
    }

    let content = std::fs::read_to_string(grub_default)?;
    // Note: If encryption is used, the theme will not be visible during the initial
    // boot phase (GRUB password prompt) because /usr is on the encrypted partition.
    let theme_path = "/usr/share/grub/themes/instantos/theme.txt";

    let mut new_lines = Vec::new();
    let mut theme_set = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("GRUB_THEME=") || trimmed.starts_with("#GRUB_THEME=") {
            new_lines.push(format!("GRUB_THEME=\"{}\"", theme_path));
            theme_set = true;
        } else {
            new_lines.push(line.to_string());
        }
    }

    if !theme_set {
        new_lines.push(format!("GRUB_THEME=\"{}\"", theme_path));
    }

    // Only write if changed to ensure idempotency
    let new_content = new_lines.join("\n");
    if new_content != content {
        std::fs::write(grub_default, new_content)?;
        println!("Updated GRUB theme configuration.");

        // Update grub config
        // Try to detect where grub-mkconfig writes to. Usually /boot/grub/grub.cfg
        let grub_cfg = "/boot/grub/grub.cfg";
        if std::path::Path::new(grub_cfg).exists() {
            println!("Regenerating GRUB configuration...");
            let mut cmd = Command::new("grub-mkconfig");
            cmd.arg("-o").arg(grub_cfg);
            executor.run(&mut cmd)?;
        }
    } else {
        println!("GRUB theme already configured.");
    }

    Ok(())
}

fn add_grub_param(content: &str, key: &str, param: &str) -> String {
    let mut new_lines = Vec::new();
    let mut found = false;
    let key_eq = format!("{}=", key);

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(&key_eq) {
            found = true;
            // Split key and value
            let parts: Vec<&str> = line.splitn(2, '=').collect();
            if parts.len() != 2 {
                new_lines.push(line.to_string());
                continue;
            }

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
                // Check if param is already present to avoid duplication
                if inner_val.contains(param) {
                    inner_val.to_string()
                } else {
                    format!("{} {}", inner_val, param)
                }
            };

            // Reconstruct with double quotes for safety
            new_lines.push(format!("{}=\"{}\"", parts[0], new_val));
        } else {
            new_lines.push(line.to_string());
        }
    }

    if !found {
        // If not found, add it
        new_lines.push(format!("{}=\"{}\"", key, param));
    }

    new_lines.join("\n")
}

fn add_grub_kernel_param(content: &str, param: &str) -> String {
    add_grub_param(content, "GRUB_CMDLINE_LINUX", param)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_grub_param() {
        let param = "test_param";
        let key = "TEST_KEY";

        // Case 1: Empty value
        let input = "TEST_KEY=\"\"";
        let expected = format!("TEST_KEY=\"{}\"", param);
        assert_eq!(add_grub_param(input, key, param), expected);

        // Case 2: Existing value
        let input = "TEST_KEY=\"existing\"";
        let expected = format!("TEST_KEY=\"existing {}\"", param);
        assert_eq!(add_grub_param(input, key, param), expected);

        // Case 3: Already present
        let input = "TEST_KEY=\"existing test_param\"";
        let expected = "TEST_KEY=\"existing test_param\"";
        assert_eq!(add_grub_param(input, key, param), expected);

        // Case 4: Not present in file
        let input = "OTHER_KEY=1";
        let expected = format!("OTHER_KEY=1\nTEST_KEY=\"{}\"", param);
        assert_eq!(add_grub_param(input, key, param), expected);

        // Case 5: Existing value with single quotes
        let input = "TEST_KEY='existing'";
        let expected = format!("TEST_KEY=\"existing {}\"", param);
        assert_eq!(add_grub_param(input, key, param), expected);

        // Case 6: No quotes
        let input = "TEST_KEY=existing";
        let expected = format!("TEST_KEY=\"existing {}\"", param);
        assert_eq!(add_grub_param(input, key, param), expected);

        // Case 7: Multiple lines
        let input = "GRUB_DEFAULT=0\nTEST_KEY=\"\"\nGRUB_TIMEOUT=5";
        let expected = format!("GRUB_DEFAULT=0\nTEST_KEY=\"{}\"\nGRUB_TIMEOUT=5", param);
        assert_eq!(add_grub_param(input, key, param), expected);
    }

    #[test]
    fn test_add_grub_kernel_param() {
        let param = "rd.luks.name=123=cryptlvm root=/dev/mapper/instantOS-root resume=/dev/mapper/instantOS-swap";
        let input = "GRUB_CMDLINE_LINUX=\"\"";
        let expected = format!("GRUB_CMDLINE_LINUX=\"{}\"", param);
        assert_eq!(add_grub_kernel_param(input, param), expected);

        let input = "GRUB_CMDLINE_LINUX=\"quiet splash\"";
        let expected = format!("GRUB_CMDLINE_LINUX=\"quiet splash {}\"", param);
        assert_eq!(add_grub_kernel_param(input, param), expected);
    }
}
