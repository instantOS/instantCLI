use super::CommandExecutor;
use anyhow::{Context, Result};
use std::process::Command;

use crate::arch::engine::{InstallContext, QuestionId};

/// Set up instantOS on a system.
///
/// This function is used by both:
/// - `ins arch install` (Post step, inside chroot after Config installed standard packages)
/// - `ins arch setup` (on existing vanilla Arch installations)
///
/// It only installs instantOS-specific packages and configuration, not standard Arch packages.
pub async fn setup_instantos(
    context: &InstallContext,
    executor: &CommandExecutor,
    override_user: Option<String>,
) -> Result<()> {
    println!("Setting up instantOS...");

    let minimal_mode = context.get_answer_bool(QuestionId::MinimalMode);

    if !minimal_mode {
        // Enable multilib for 32-bit support (Steam, Wine, etc.)
        // This is idempotent - only enables if not already enabled
        println!("Enabling multilib repository...");
        crate::common::pacman::enable_multilib(executor.dry_run).await?;

        // Set up instantOS repository and install instantOS packages
        setup_instant_repo(executor).await?;
        install_instant_packages(context, executor)?;

        // Update /etc/os-release to identify as instantOS
        update_os_release(executor)?;

        // Configure GRUB theme
        crate::arch::execution::bootloader::configure_grub_theme(context, executor)?;
    }

    // Determine username: override > context > SUDO_USER
    let username = override_user.or_else(|| context.get_answer(&QuestionId::Username).cloned());

    // Configure user groups (create groups and add user to them)
    // This reuses the same functions as ins arch install for consistency
    println!("Configuring user groups...");
    super::config::ensure_groups_exist(executor)?;
    if let Some(user) = username.as_ref() {
        super::config::add_user_to_groups(user, executor)?;
    } else {
        println!("No username provided, skipping user group membership.");
    }

    if !minimal_mode {
        if let Some(user) = username.clone() {
            setup_user_dotfiles(&user, executor)?;
            setup_wallpaper(&user, executor)?;
        } else {
            println!("Skipping dotfiles setup: No user specified and SUDO_USER not found.");
        }
    }

    setup_backlight_udev_rule(executor)?;
    enable_services(executor, context)?;
    super::config::configure_environment(executor)?;

    Ok(())
}

/// Set up the instantOS repository in pacman.conf.
///
/// Note: This does NOT enable multilib. For fresh installations, multilib is enabled
/// during the Config step. For `ins arch setup` on existing systems, users already
/// have their own multilib configuration.
pub async fn setup_instant_repo(executor: &CommandExecutor) -> Result<()> {
    println!("Setting up instantOS repository...");
    crate::common::pacman::setup_instant_repo(executor.dry_run).await?;

    // Update repositories to include [instant]
    println!("Updating repositories...");
    let mut cmd = Command::new("pacman");
    cmd.arg("-Sy");
    executor.run(&mut cmd)?;

    Ok(())
}

/// Install instantOS packages from the [instant] repository.
///
/// These are the only packages installed by `ins arch setup` on existing systems.
/// For fresh installations, standard packages are installed separately in the Config step.
fn install_instant_packages(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    let packages = crate::arch::execution::packages::build_instant_package_plan(context);
    if packages.is_empty() {
        println!("Minimal mode enabled, skipping instantOS packages.");
        return Ok(());
    }
    println!("Installing instantOS packages: {}", packages.join(", "));
    let package_refs: Vec<&str> = packages.iter().map(|s| s.as_str()).collect();
    super::pacman::install(&package_refs, executor)?;
    Ok(())
}

fn setup_user_dotfiles(username: &str, executor: &CommandExecutor) -> Result<()> {
    println!("Setting up dotfiles for user: {}", username);

    // Check if dotfiles repo already exists
    let check_cmd_str = "ins dot repo list";
    let mut cmd_check = Command::new("su");
    cmd_check.arg("-c").arg(check_cmd_str).arg(username);

    let repo_exists = if let Some(output) = executor.run_with_output(&mut cmd_check)? {
        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout.contains("dotfiles")
    } else {
        false
    };

    if !repo_exists {
        // Clone dotfiles
        // su -c "ins dot repo clone https://github.com/instantOS/dotfiles --read-only" username
        // TODO: make repo url a constant
        let clone_cmd_str = "ins dot repo clone https://github.com/instantOS/dotfiles --read-only";
        let mut cmd_clone = Command::new("su");
        cmd_clone.arg("-c").arg(clone_cmd_str).arg(username);

        executor.run(&mut cmd_clone)?;
    } else {
        println!("Dotfiles repository already exists, skipping clone.");
    }

    // Apply dotfiles
    // su -c "ins dot apply" username
    let apply_cmd_str = "ins dot apply";
    let mut cmd_apply = Command::new("su");
    cmd_apply.arg("-c").arg(apply_cmd_str).arg(username);

    executor.run(&mut cmd_apply)?;

    // Change shell to zsh
    // chsh -s /bin/zsh username
    let mut cmd_chsh = Command::new("chsh");
    cmd_chsh.arg("-s").arg("/bin/zsh").arg(username);

    executor.run(&mut cmd_chsh)?;

    Ok(())
}

fn setup_wallpaper(username: &str, executor: &CommandExecutor) -> Result<()> {
    println!("Setting up wallpaper for user: {}", username);

    // Run `ins wallpaper random` as the user
    let wallpaper_cmd_str = "ins wallpaper random";
    let mut cmd = Command::new("su");
    cmd.arg("-c").arg(wallpaper_cmd_str).arg(username);

    executor.run(&mut cmd)?;

    Ok(())
}

fn enable_services(executor: &CommandExecutor, context: &InstallContext) -> Result<()> {
    println!("Enabling services...");

    let mut services = vec!["NetworkManager", "sshd"];

    // Enable VM-specific services
    if let Some(vm_type) = &context.system_info.vm_type {
        match vm_type.as_str() {
            "vmware" => {
                services.push("vmtoolsd");
            }
            "kvm" | "qemu" | "bochs" => {
                services.push("qemu-guest-agent");
            }
            "oracle" => {
                services.push("vboxservice");
            }
            _ => {}
        }
    }

    // Check if other display managers are enabled
    // We check this directly via Command because CommandExecutor errors on failure (non-zero exit),
    // and systemctl is-enabled returns non-zero if disabled.
    let mut other_dm_enabled = false;

    for dm in &["sddm", "gdm"] {
        let mut cmd = Command::new("systemctl");
        cmd.arg("is-enabled").arg(dm);
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());

        if let Ok(status) = cmd.status()
            && status.success()
        {
            println!("Detected enabled display manager: {}", dm);
            other_dm_enabled = true;
            break;
        }
    }

    if !other_dm_enabled && !context.get_answer_bool(QuestionId::MinimalMode) {
        services.push("lightdm");

        // Handle Autologin
        let enable_autologin = context.get_answer_bool(QuestionId::Autologin);

        if enable_autologin {
            configure_lightdm_autologin(context, executor)?;
        }
    } else {
        println!("Skipping lightdm setup because another display manager is enabled.");
    }

    for service in services {
        let mut cmd = Command::new("systemctl");
        cmd.arg("enable").arg(service);
        executor.run(&mut cmd)?;
    }

    Ok(())
}

fn update_os_release(executor: &CommandExecutor) -> Result<()> {
    println!("Updating /etc/os-release...");

    if executor.dry_run {
        println!("[DRY RUN] Update /etc/os-release with instantOS values");
        return Ok(());
    }

    let path = std::path::Path::new("/etc/os-release");
    if !path.exists() {
        println!("Warning: /etc/os-release not found");
        return Ok(());
    }

    let content = std::fs::read_to_string(path)?;
    let mut new_lines = Vec::new();

    let mut found_name = false;
    let mut found_id = false;
    let mut found_pretty_name = false;
    let mut found_id_like = false;

    for line in content.lines() {
        if line.starts_with("NAME=") {
            new_lines.push("NAME=\"instantOS\"".to_string());
            found_name = true;
        } else if line.starts_with("ID=") {
            new_lines.push("ID=\"instantos\"".to_string());
            found_id = true;
        } else if line.starts_with("PRETTY_NAME=") {
            new_lines.push("PRETTY_NAME=\"instantOS\"".to_string());
            found_pretty_name = true;
        } else if line.starts_with("ID_LIKE=") {
            new_lines.push("ID_LIKE=\"arch\"".to_string());
            found_id_like = true;
        } else {
            new_lines.push(line.to_string());
        }
    }

    if !found_name {
        new_lines.push("NAME=\"instantOS\"".to_string());
    }
    if !found_id {
        new_lines.push("ID=\"instantos\"".to_string());
    }
    if !found_pretty_name {
        new_lines.push("PRETTY_NAME=\"instantOS\"".to_string());
    }
    if !found_id_like {
        new_lines.push("ID_LIKE=\"arch\"".to_string());
    }

    let new_content = new_lines.join("\n");

    // Only write if changed
    if new_content != content {
        std::fs::write(path, new_content)?;
        println!("Updated /etc/os-release");
    } else {
        println!("/etc/os-release already up to date");
    }

    Ok(())
}

fn configure_lightdm_autologin(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    println!("Configuring LightDM autologin...");

    let username = context
        .get_answer(&QuestionId::Username)
        .context("Username not set for autologin")?;

    if executor.dry_run {
        println!("[DRY RUN] Enable autologin for user: {}", username);
        return Ok(());
    }

    let config_path = "/etc/lightdm/lightdm.conf";
    if !std::path::Path::new(config_path).exists() {
        println!(
            "Warning: {} not found, cannot configure autologin",
            config_path
        );
        return Ok(());
    }

    // Enable autologin-user
    let content = std::fs::read_to_string(config_path)?;
    let new_content = update_lightdm_conf(&content, username);

    if content != new_content {
        std::fs::write(config_path, new_content)?;
        println!("Updated lightdm.conf with autologin settings");
    } else {
        println!("lightdm.conf already configured or keys not found");
    }

    Ok(())
}

fn update_lightdm_conf(content: &str, username: &str) -> String {
    let mut new_lines = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim_start();
        // Check for autologin-user (commented or not)
        if trimmed.starts_with("autologin-user=") || trimmed.starts_with("#autologin-user=") {
            new_lines.push(format!("autologin-user={}", username));
        }
        // Check for autologin-user-timeout (commented or not)
        else if trimmed.starts_with("autologin-user-timeout=")
            || trimmed.starts_with("#autologin-user-timeout=")
        {
            new_lines.push("autologin-user-timeout=0".to_string());
        } else {
            new_lines.push(line.to_string());
        }
    }

    new_lines.join("\n")
}

fn setup_backlight_udev_rule(executor: &CommandExecutor) -> Result<()> {
    println!("Configuring backlight udev rules...");

    if executor.dry_run {
        println!("[DRY RUN] Create /etc/udev/rules.d/90-backlight.rules");
        return Ok(());
    }

    let rules_path = "/etc/udev/rules.d/90-backlight.rules";
    let rules_content = r#"ACTION=="add", SUBSYSTEM=="backlight", RUN+="/bin/chgrp video $sys$devpath/brightness", RUN+="/bin/chmod g+w $sys$devpath/brightness""#;

    // Ensure parent directory exists
    if let Some(parent) = std::path::Path::new(rules_path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(rules_path, rules_content)?;
    println!("Created {}", rules_path);

    // Try to reload udev rules (ignore errors as it might fail in chroot)
    let mut cmd = Command::new("udevadm");
    cmd.arg("control").arg("--reload-rules");
    let _ = executor.run(&mut cmd);

    let mut cmd_trigger = Command::new("udevadm");
    cmd_trigger.arg("trigger");
    let _ = executor.run(&mut cmd_trigger);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_lightdm_conf() {
        let input = r#"
[Seat:*]
#autologin-guest=false
#autologin-user=
#autologin-user-timeout=0
"#;
        let expected = r#"
[Seat:*]
#autologin-guest=false
autologin-user=testuser
autologin-user-timeout=0
"#;
        let result = update_lightdm_conf(input, "testuser");
        assert_eq!(result.trim(), expected.trim());
    }

    #[test]
    fn test_update_lightdm_conf_already_set() {
        let input = "autologin-user=olduser\nautologin-user-timeout=5";
        let expected = "autologin-user=newuser\nautologin-user-timeout=0";
        let result = update_lightdm_conf(input, "newuser");
        assert_eq!(result, expected);
    }
}
