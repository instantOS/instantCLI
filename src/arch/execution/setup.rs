use super::CommandRunner;
use anyhow::{Context, Result};
use std::process::Command;

use crate::arch::engine::{InstallContext, StepId};
use crate::common::config_edit::{set_keys, set_keys_in_section, update_file};

/// URL for the instantOS dotfiles repository
const INSTANTOS_DOTFILES_REPO: &str = "https://github.com/instantOS/dotfiles";

/// Set up instantOS on a system.
///
/// This function is used by both:
/// - `ins arch install` (Post step, inside chroot after Config installed standard packages)
/// - `ins arch setup` (on existing vanilla Arch installations)
///
/// It only installs instantOS-specific packages and configuration, not standard Arch packages.
pub async fn setup_instantos(
    context: &InstallContext,
    executor: &dyn CommandRunner,
    override_user: Option<String>,
) -> Result<()> {
    println!("Setting up instantOS...");

    let minimal_mode = context.get_answer_bool(StepId::MinimalMode);

    if !minimal_mode {
        // Enable multilib for 32-bit support (Steam, Wine, etc.)
        // This is idempotent - only enables if not already enabled
        println!("Enabling multilib repository...");
        crate::common::pacman::enable_multilib(executor.dry_run()).await?;

        // Set up instantOS repository and install instantOS packages
        setup_instant_repo(executor).await?;
        install_instant_packages(context, executor)?;

        // Configure Plymouth theme (after instantOS packages are installed)
        super::config::configure_plymouth(context, executor)?;

        // Update /etc/os-release to identify as instantOS
        update_os_release(executor)?;

        // Configure GRUB theme
        crate::arch::execution::bootloader::configure_grub_theme(context, executor)?;
    }

    // Determine username: override > context > SUDO_USER
    let username = override_user.or_else(|| context.get_answer(&StepId::Username).cloned());

    // Configure user groups (create groups and add user to them)
    // This reuses the same functions as ins arch install for consistency
    println!("Configuring user groups...");
    super::config::ensure_groups_exist(executor)?;
    if let Some(user) = username.as_ref() {
        super::config::add_user_to_groups(user, executor)?;

        // Create standard XDG user directories (Desktop, Documents, etc.)
        println!("Creating XDG user directories for {}...", user);
        let mut cmd_xdg = Command::new("su");
        cmd_xdg.arg("-c").arg("xdg-user-dirs-update").arg(user);
        let _ = executor.run(&mut cmd_xdg);
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
pub async fn setup_instant_repo(executor: &dyn CommandRunner) -> Result<()> {
    println!("Setting up instantOS repository...");
    crate::common::pacman::setup_instant_repo(executor.dry_run()).await?;

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
fn install_instant_packages(context: &InstallContext, executor: &dyn CommandRunner) -> Result<()> {
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

fn setup_user_dotfiles(username: &str, executor: &dyn CommandRunner) -> Result<()> {
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
        let clone_cmd_str = format!("ins dot repo clone {} --read-only", INSTANTOS_DOTFILES_REPO);
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

fn setup_wallpaper(username: &str, executor: &dyn CommandRunner) -> Result<()> {
    println!("Setting up wallpaper for user: {}", username);

    // Run `ins wallpaper random` as the user
    let wallpaper_cmd_str = "ins wallpaper random";
    let mut cmd = Command::new("su");
    cmd.arg("-c").arg(wallpaper_cmd_str).arg(username);

    executor.run(&mut cmd)?;

    Ok(())
}

fn enable_services(executor: &dyn CommandRunner, context: &InstallContext) -> Result<()> {
    println!("Enabling services...");

    let mut services = vec!["NetworkManager", "sshd", "systemd-timesyncd"];
    let desktop = crate::arch::config::DesktopEnvironment::from_context(context);

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

    let selected_dm = crate::arch::config::DisplayManager::from_context(context);
    let selected_dm_service = selected_dm.answer_value();

    // Check if other display managers are enabled
    // We check this directly via Command because the executor errors on failure (non-zero exit),
    // and systemctl is-enabled returns non-zero if disabled.
    let mut other_dm_enabled = false;

    for check_dm in &["sddm", "gdm", "lightdm"] {
        if *check_dm == selected_dm_service {
            continue;
        }
        let mut cmd = Command::new("systemctl");
        cmd.arg("is-enabled").arg(check_dm);
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());

        if let Ok(status) = cmd.status()
            && status.success()
        {
            println!("Detected enabled display manager: {}", check_dm);
            other_dm_enabled = true;
            break;
        }
    }

    if !other_dm_enabled
        && !context.get_answer_bool(StepId::MinimalMode)
        && desktop.requires_display_manager()
    {
        services.push(selected_dm_service);

        match selected_dm {
            crate::arch::config::DisplayManager::Gdm => {
                configure_gdm_session(context, executor)?;
                if context.get_answer_bool(StepId::Autologin) {
                    configure_gdm_autologin(context, executor)?;
                }
            }
            crate::arch::config::DisplayManager::Lightdm => {
                configure_lightdm_session(context, executor)?;
                if context.get_answer_bool(StepId::Autologin) {
                    configure_lightdm_autologin(context, executor)?;
                }
            }
        }
    } else if other_dm_enabled {
        println!(
            "Skipping {} setup because another display manager is enabled.",
            selected_dm_service
        );
    } else if context.get_answer_bool(StepId::MinimalMode) {
        println!(
            "Skipping {} setup because minimal mode is enabled.",
            selected_dm_service
        );
    } else {
        println!(
            "Skipping {} setup because no graphical desktop was selected.",
            selected_dm_service
        );
    }

    for service in services {
        let mut cmd = Command::new("systemctl");
        cmd.arg("enable").arg(service);
        executor.run(&mut cmd)?;
    }

    // Enable the user-level SSH agent socket (socket-activated via ~/.config/systemd/user/)
    println!("Enabling user-level ssh-agent.socket...");
    let mut cmd = Command::new("systemctl");
    cmd.arg("--global").arg("enable").arg("ssh-agent.socket");
    executor.run(&mut cmd)?;

    Ok(())
}

fn update_os_release(executor: &dyn CommandRunner) -> Result<()> {
    println!("Updating /etc/os-release...");

    if executor.dry_run() {
        println!("[DRY RUN] Update /etc/os-release with instantOS values");
        return Ok(());
    }

    let path = "/etc/os-release";
    if !std::path::Path::new(path).exists() {
        println!("Warning: /etc/os-release not found");
        return Ok(());
    }

    let changed = update_file(path, |content| {
        set_keys(
            content,
            &[
                ("NAME", "\"instantOS\""),
                ("ID", "\"instantos\""),
                ("PRETTY_NAME", "\"instantOS\""),
                ("ID_LIKE", "\"arch\""),
            ],
        )
    })?;

    if changed {
        println!("Updated /etc/os-release");
    } else {
        println!("/etc/os-release already up to date");
    }

    Ok(())
}

fn configure_lightdm_session(context: &InstallContext, executor: &dyn CommandRunner) -> Result<()> {
    let desktop = crate::arch::config::DesktopEnvironment::from_context(context);
    let Some(session_name) = desktop.session_name() else {
        return Ok(());
    };

    println!("Configuring LightDM default session to {}...", session_name);

    if executor.dry_run() {
        println!(
            "[DRY RUN] Set LightDM user-session and autologin-session to {}",
            session_name
        );
        return Ok(());
    }

    let config_path = "/etc/lightdm/lightdm.conf";
    if !std::path::Path::new(config_path).exists() {
        println!(
            "Warning: {} not found, cannot configure LightDM defaults",
            config_path
        );
        return Ok(());
    }

    let changed = update_file(config_path, |content| {
        set_keys_in_section(
            content,
            "Seat:*",
            &[
                ("user-session", session_name),
                ("autologin-session", session_name),
            ],
        )
    })?;

    if changed {
        println!("Updated lightdm.conf with default session settings");
    } else {
        println!("lightdm.conf already configured for the selected session");
    }

    Ok(())
}

fn configure_lightdm_autologin(
    context: &InstallContext,
    executor: &dyn CommandRunner,
) -> Result<()> {
    println!("Configuring LightDM autologin...");

    let username = context
        .get_answer(&StepId::Username)
        .context("Username not set for autologin")?;
    let session_name =
        crate::arch::config::DesktopEnvironment::from_context(context).session_name();

    if executor.dry_run() {
        println!("[DRY RUN] Enable autologin for user: {}", username);
        if let Some(session_name) = session_name {
            println!(
                "[DRY RUN] Set LightDM autologin-session to {}",
                session_name
            );
        }
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

    let changed = update_file(config_path, |content| {
        let mut keys = vec![
            ("autologin-user", username.as_str()),
            ("autologin-user-timeout", "0"),
        ];
        if let Some(session_name) = session_name {
            keys.push(("autologin-session", session_name));
        }
        set_keys_in_section(content, "Seat:*", &keys)
    })?;

    if changed {
        println!("Updated lightdm.conf with autologin settings");
    } else {
        println!("lightdm.conf already configured or keys not found");
    }

    Ok(())
}

fn setup_backlight_udev_rule(executor: &dyn CommandRunner) -> Result<()> {
    println!("Configuring backlight udev rules...");

    if executor.dry_run() {
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

fn configure_gdm_session(context: &InstallContext, executor: &dyn CommandRunner) -> Result<()> {
    let desktop = crate::arch::config::DesktopEnvironment::from_context(context);
    let Some(session_name) = desktop.gdm_session_name() else {
        return Ok(());
    };
    let username = context
        .get_answer(&StepId::Username)
        .context("Username not set for GDM session configuration")?;

    println!(
        "Configuring GDM default session for {} to {}...",
        username, session_name
    );

    if executor.dry_run() {
        println!(
            "[DRY RUN] Set GDM user session for {} to {} in AccountsService",
            username, session_name
        );
        return Ok(());
    }

    let dir_path = "/var/lib/AccountsService/users";
    let file_path = format!("{}/{}", dir_path, username);

    // Create the directory if it doesn't exist
    std::fs::create_dir_all(dir_path)?;

    let content = if std::path::Path::new(&file_path).exists() {
        std::fs::read_to_string(&file_path)?
    } else {
        String::new()
    };

    let edit = set_keys_in_section(&content, "User", &[("Session", session_name)]);
    if edit.changed {
        std::fs::write(&file_path, edit.content)?;
    }
    Ok(())
}

fn configure_gdm_autologin(context: &InstallContext, executor: &dyn CommandRunner) -> Result<()> {
    println!("Configuring GDM autologin...");

    let username = context
        .get_answer(&StepId::Username)
        .context("Username not set for GDM autologin")?;

    if executor.dry_run() {
        println!(
            "[DRY RUN] Enable GDM autologin for user: {} in /etc/gdm/custom.conf",
            username
        );
        return Ok(());
    }

    let config_path = "/etc/gdm/custom.conf";
    if !std::path::Path::new(config_path).exists() {
        println!(
            "Warning: {} not found, cannot configure GDM autologin",
            config_path
        );
        return Ok(());
    }

    let changed = update_file(config_path, |content| {
        set_keys_in_section(
            content,
            "daemon",
            &[
                ("AutomaticLoginEnable", "true"),
                ("AutomaticLogin", username),
            ],
        )
    })?;

    if changed {
        println!("Updated custom.conf with GDM autologin settings");
    } else {
        println!("custom.conf already configured for GDM autologin");
    }

    Ok(())
}
