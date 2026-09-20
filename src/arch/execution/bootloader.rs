use super::CommandRunner;
use crate::arch::engine::{BootMode, InstallPlan};
use crate::common::config_edit::set_keys;
use anyhow::Result;
use std::process::Command;

pub async fn install_bootloader(plan: &InstallPlan, executor: &dyn CommandRunner) -> Result<()> {
    println!("Installing bootloader (inside chroot)...");

    match plan.system_info.boot_mode {
        BootMode::UEFI64 | BootMode::UEFI32 => install_grub_uefi(plan, executor)?,
        BootMode::BIOS => install_grub_bios(plan, executor)?,
    }

    configure_grub(plan, executor)?;

    Ok(())
}

/// Packages needed for bootloader setup (installed in a single batch elsewhere)
pub fn bootloader_package_list(plan: &InstallPlan) -> Vec<String> {
    let mut packages = vec!["grub".to_string(), "os-prober".to_string()];

    if matches!(
        plan.system_info.boot_mode,
        BootMode::UEFI64 | BootMode::UEFI32
    ) {
        packages.push("efibootmgr".to_string());
    }

    packages
}

fn install_grub_uefi(plan: &InstallPlan, executor: &dyn CommandRunner) -> Result<()> {
    println!("Detected UEFI mode. Installing GRUB for UEFI...");

    // Determine the appropriate target based on UEFI mode
    let target = match plan.system_info.boot_mode {
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

fn install_grub_bios(plan: &InstallPlan, executor: &dyn CommandRunner) -> Result<()> {
    println!("Detected BIOS mode. Installing GRUB for BIOS...");

    // disk is now just the device path (e.g., "/dev/sda")
    let disk = plan.storage.disk().as_str();

    println!("Installing GRUB to MBR of {}", disk);

    // grub-install --target=i386-pc /dev/sdX
    let mut cmd = Command::new("grub-install");
    cmd.arg("--target=i386-pc").arg(disk);

    executor.run(&mut cmd)?;

    Ok(())
}

fn configure_grub(plan: &InstallPlan, executor: &dyn CommandRunner) -> Result<()> {
    println!("Generating GRUB configuration...");

    if plan.storage.encryption().is_some() {
        configure_grub_encryption(plan, executor)?;
    }

    if plan.use_plymouth && !plan.minimal_mode {
        configure_grub_plymouth(executor)?;
    }

    if !plan.minimal_mode {
        // Apply the theme to /etc/default/grub only; the single grub-mkconfig
        // below picks it up together with every other edit made here. A fresh
        // install used to regenerate the GRUB configuration once with the
        // theme and once more right after — pure duplicate work.
        configure_grub_theme(executor, false)?;
    }

    // grub-mkconfig -o /boot/grub/grub.cfg
    let mut cmd = Command::new("grub-mkconfig");
    cmd.arg("-o").arg("/boot/grub/grub.cfg");

    executor.run(&mut cmd)?;

    if executor.dry_run() {
        // The check below reads the generated file, which does not exist in
        // a dry run.
        return Ok(());
    }

    // grub-mkconfig can exit successfully while leaving an empty or
    // entry-less configuration behind; such a system cannot boot. Fail
    // loudly instead of reporting a finished installation.
    let grub_cfg = std::fs::read_to_string("/boot/grub/grub.cfg").unwrap_or_else(|error| {
        println!("Warning: could not read generated grub.cfg: {error}");
        String::new()
    });
    if !grub_cfg.contains("menuentry") {
        anyhow::bail!(
            "grub-mkconfig produced a boot configuration without menu entries ({} bytes); the system would not boot",
            grub_cfg.len()
        );
    }

    Ok(())
}

fn configure_grub_encryption(plan: &InstallPlan, executor: &dyn CommandRunner) -> Result<()> {
    if executor.dry_run() {
        println!("[DRY RUN] Adding 'rd.luks.name=...=cryptlvm' to GRUB_CMDLINE_LINUX");
        println!("[DRY RUN] Setting GRUB_ENABLE_CRYPTODISK=y in /etc/default/grub");
        return Ok(());
    }

    let luks_part = luks_partition_path(plan);
    let uuid = read_luks_uuid(&luks_part)?;
    println!("Found LUKS UUID: {}", uuid);

    let grub_default = "/etc/default/grub";
    let content = std::fs::read_to_string(grub_default)?;
    let param = build_grub_encryption_param(&uuid);
    let with_param = add_grub_kernel_param(&content, &param);
    // Reactivates the stock commented `#GRUB_ENABLE_CRYPTODISK=y` default.
    let edit = set_keys(&with_param, &[("GRUB_ENABLE_CRYPTODISK", "y")]);
    std::fs::write(grub_default, edit.content)?;

    Ok(())
}

fn luks_partition_path(plan: &InstallPlan) -> String {
    crate::arch::execution::disk::get_part_path(plan.storage.disk().as_str(), 2)
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

fn configure_grub_plymouth(executor: &dyn CommandRunner) -> Result<()> {
    if executor.dry_run() {
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

/// Point GRUB at the instantOS theme in /etc/default/grub.
///
/// When `regenerate_config` is true and the theme changed, the GRUB
/// configuration is regenerated right away. Callers that run
/// `grub-mkconfig` themselves after applying all /etc/default/grub edits
/// (the Bootloader step does) pass `false` so the expensive regeneration
/// happens exactly once.
pub fn configure_grub_theme(executor: &dyn CommandRunner, regenerate_config: bool) -> Result<()> {
    let grub_default = "/etc/default/grub";

    if !std::path::Path::new(grub_default).exists() {
        println!("GRUB configuration file not found, skipping theme setup.");
        return Ok(());
    }

    if executor.dry_run() {
        println!("[DRY RUN] Setting GRUB_THEME in /etc/default/grub");
        return Ok(());
    }

    let content = std::fs::read_to_string(grub_default)?;
    // Note: If encryption is used, the theme will not be visible during the initial
    // boot phase (GRUB password prompt) because /usr is on the encrypted partition.
    let theme_path = "/usr/share/grub/themes/instantos/theme.txt";

    let edit = set_keys(&content, &[("GRUB_THEME", &format!("\"{theme_path}\""))]);

    // Only write if changed to ensure idempotency
    if edit.changed {
        std::fs::write(grub_default, &edit.content)?;
        println!("Updated GRUB theme configuration.");

        // Update grub config
        // Try to detect where grub-mkconfig writes to. Usually /boot/grub/grub.cfg
        let grub_cfg = "/boot/grub/grub.cfg";
        if regenerate_config && std::path::Path::new(grub_cfg).exists() {
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
