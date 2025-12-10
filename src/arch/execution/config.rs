use super::CommandExecutor;
use crate::arch::engine::{InstallContext, QuestionId};
use crate::arch::mkinitcpio::MkinitcpioConfig;
use anyhow::{Context, Result};
use std::process::Command;

pub async fn install_config(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    println!("Configuring system (inside chroot)...");

    configure_pacman_target(executor).await?;
    super::setup::install_all_packages(context, executor)?;
    configure_timezone(context, executor)?;
    configure_locale(context, executor)?;
    configure_network(context, executor)?;
    configure_users(context, executor)?;
    configure_vconsole(context, executor)?;
    configure_sudo(context, executor)?;
    configure_mkinitcpio(context, executor)?;
    if !context.get_answer_bool(QuestionId::MinimalMode) {
        configure_plymouth(context, executor)?;
    }

    Ok(())
}

async fn configure_pacman_target(executor: &CommandExecutor) -> Result<()> {
    println!("Configuring target pacman settings...");
    crate::common::pacman::configure_pacman_settings(Some("/etc/pacman.conf"), executor.dry_run)
        .await?;
    Ok(())
}

/// Packages required for configuration steps (installed in a single batch elsewhere)
pub fn config_package_list(context: &InstallContext) -> Vec<String> {
    let mut packages = Vec::new();

    if context.get_answer_bool(QuestionId::UseEncryption) {
        packages.push("lvm2".to_string());
        packages.push("cryptsetup".to_string());
    }

    if context.get_answer_bool(QuestionId::UsePlymouth)
        && !context.get_answer_bool(QuestionId::MinimalMode)
    {
        packages.push("plymouth".to_string());
    }

    packages
}

fn configure_mkinitcpio(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    let use_encryption = context.get_answer_bool(QuestionId::UseEncryption);
    let use_plymouth = context.get_answer_bool(QuestionId::UsePlymouth);

    if !use_encryption && !use_plymouth {
        return Ok(());
    }

    if use_encryption {
        println!("Configuring mkinitcpio for encryption...");
    }
    if use_plymouth && !context.get_answer_bool(QuestionId::MinimalMode) {
        println!("Configuring mkinitcpio for Plymouth...");
    }

    if executor.dry_run {
        if use_plymouth && !context.get_answer_bool(QuestionId::MinimalMode) {
            println!("[DRY RUN] Adding 'plymouth' to HOOKS in /etc/mkinitcpio.conf");
        }
        if use_encryption {
            println!("[DRY RUN] Adding 'encrypt lvm2' to HOOKS in /etc/mkinitcpio.conf");
        }
        println!("[DRY RUN] mkinitcpio -P");
        return Ok(());
    }

    let conf_path = "/etc/mkinitcpio.conf";
    let content = std::fs::read_to_string(conf_path).context("Failed to read mkinitcpio.conf")?;

    let mut config = MkinitcpioConfig::parse(&content)?;

    // Switch to systemd hooks
    if config.contains_hook("base") && config.contains_hook("udev") {
        config.replace_hook("udev", "systemd");
    }

    // Plymouth should be after systemd but before encrypt/sd-encrypt
    // And definitely before sd-encrypt to show password prompt
    if use_plymouth && !context.get_answer_bool(QuestionId::MinimalMode) {
        config.ensure_hook_position(
            "plymouth",
            &["base", "systemd", "udev"],       // After these
            &["sd-encrypt", "encrypt", "lvm2"], // Before these
        );
    }

    // Ensure keyboard and keymap/sd-vconsole are present
    if !config.contains_hook("sd-vconsole") {
        if config.contains_hook("keymap") {
            config.replace_hook("keymap", "sd-vconsole");
        } else {
            // Ensure sd-vconsole comes after keyboard (or just add it if keyboard not present)
            config.ensure_hook_position("sd-vconsole", &["keyboard"], &[]);
        }
    }

    // Remove consolefont if present as sd-vconsole handles it
    config.remove_hook("consolefont");

    // Add encryption hooks in correct order: block -> sd-encrypt -> lvm2 -> resume -> filesystems
    if use_encryption && config.contains_hook("block") && config.contains_hook("filesystems") {
        // Replace legacy encrypt hook if present
        if config.contains_hook("encrypt") {
            config.replace_hook("encrypt", "sd-encrypt");
        } else {
            config.ensure_hook("sd-encrypt");
        }

        // Ensure correct ordering for full disk encryption with LVM and resume
        config.ensure_hook_position("sd-encrypt", &["block"], &["filesystems"]);
        config.ensure_hook_position("lvm2", &["sd-encrypt"], &["filesystems"]);
        config.ensure_hook_position("resume", &["lvm2"], &["filesystems"]);
    }

    std::fs::write(conf_path, config.to_string())?;

    // Regenerate initramfs
    executor.run(Command::new("mkinitcpio").arg("-P"))?;

    Ok(())
}

fn configure_vconsole(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    let keymap = context
        .get_answer(&QuestionId::Keymap)
        .context("Keymap not selected")?;

    println!("Setting console keymap to {}", keymap);

    if executor.dry_run {
        println!("[DRY RUN] echo 'KEYMAP={}' > /etc/vconsole.conf", keymap);
    } else {
        std::fs::write("/etc/vconsole.conf", format!("KEYMAP={}\n", keymap))?;
    }

    Ok(())
}

fn configure_timezone(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    let timezone = context
        .get_answer(&QuestionId::Timezone)
        .context("Timezone not selected")?;

    println!("Setting timezone to {}", timezone);

    // Try timedatectl first
    // timedatectl set-timezone "$REGION"
    let mut cmd = Command::new("timedatectl");
    cmd.arg("set-timezone").arg(timezone);

    // We try to run timedatectl. If it fails (e.g. in chroot without dbus), we fallback.
    // We suppress the error from executor.run by checking the result.
    if executor.run(&mut cmd).is_ok() {
        // timedatectl set-ntp true
        let mut cmd_ntp = Command::new("timedatectl");
        cmd_ntp.arg("set-ntp").arg("true");
        // We ignore errors here as NTP might not be controllable in chroot
        let _ = executor.run(&mut cmd_ntp);
    } else {
        println!("timedatectl failed, falling back to manual configuration...");

        // ln -sf /usr/share/zoneinfo/Region/City /etc/localtime
        let source = format!("/usr/share/zoneinfo/{}", timezone);
        let target = "/etc/localtime";

        if executor.dry_run {
            println!("[DRY RUN] ln -sf {} {}", source, target);
        } else {
            // Remove existing link/file if it exists to avoid error
            if std::path::Path::new(target).exists() {
                std::fs::remove_file(target)?;
            }
            std::os::unix::fs::symlink(&source, target)?;
        }

        // hwclock --systohc
        let mut cmd_hw = Command::new("hwclock");
        cmd_hw.arg("--systohc");
        executor.run(&mut cmd_hw)?;
    }

    Ok(())
}

fn configure_locale(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    let locale = context
        .get_answer(&QuestionId::Locale)
        .context("Locale not selected")?;

    println!("Setting locale to {}", locale);

    if executor.dry_run {
        println!("[DRY RUN] Uncommenting {} in /etc/locale.gen", locale);
        println!("[DRY RUN] locale-gen");
        // Extract just the LANG part, e.g., "en_US.UTF-8" from "en_US.UTF-8 UTF-8"
        let lang = locale.split_whitespace().next().unwrap_or(locale);
        println!("[DRY RUN] localectl set-locale LANG={}", lang);
    } else {
        // Read /etc/locale.gen
        let locale_gen_path = "/etc/locale.gen";
        let content =
            std::fs::read_to_string(locale_gen_path).context("Failed to read /etc/locale.gen")?;

        // Uncomment the selected locale
        let mut new_lines = Vec::new();
        let mut found = false;

        for line in content.lines() {
            if line.contains(locale) && line.trim().starts_with('#') {
                // Uncomment it
                new_lines.push(line.replacen('#', "", 1));
                found = true;
            } else if line.contains(locale) && !line.trim().starts_with('#') {
                // Already uncommented
                new_lines.push(line.to_string());
                found = true;
            } else {
                new_lines.push(line.to_string());
            }
        }

        if !found {
            // Append it if not found
            new_lines.push(locale.clone());
        }

        std::fs::write(locale_gen_path, new_lines.join("\n"))?;

        // Run locale-gen
        let mut cmd = Command::new("locale-gen");
        executor.run(&mut cmd)?;

        // Use localectl to set the system locale instead of directly editing /etc/locale.conf
        // Extract just the LANG part, e.g., "en_US.UTF-8" from "en_US.UTF-8 UTF-8"
        let lang = locale.split_whitespace().next().unwrap_or(locale);
        let mut cmd = Command::new("localectl");
        cmd.arg("set-locale").arg(format!("LANG={}", lang));
        executor.run(&mut cmd)?;
    }

    Ok(())
}

fn configure_network(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    let hostname = context
        .get_answer(&QuestionId::Hostname)
        .context("Hostname not set")?;

    println!("Setting hostname to {}", hostname);

    if executor.dry_run {
        println!("[DRY RUN] echo '{}' > /etc/hostname", hostname);
        println!("[DRY RUN] Writing /etc/hosts");
    } else {
        std::fs::write("/etc/hostname", format!("{}\n", hostname))?;

        let hosts_content = format!(
            "127.0.0.1\tlocalhost\n::1\t\tlocalhost\n127.0.1.1\t{}.localdomain\t{}\n",
            hostname, hostname
        );
        std::fs::write("/etc/hosts", hosts_content)?;
    }

    Ok(())
}

fn configure_users(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    let username = context
        .get_answer(&QuestionId::Username)
        .context("Username not set")?;
    let password = context
        .get_answer(&QuestionId::Password)
        .context("Password not set")?;

    println!("Configuring user: {}", username);

    // Set root password
    // echo "root:password" | chpasswd
    let root_input = format!("root:{}", password);
    let mut cmd_root = Command::new("chpasswd");
    executor.run_with_input(&mut cmd_root, &root_input)?;

    // Create user
    let groups = vec!["wheel", "video", "docker", "sys", "rfkill"];

    // Ensure groups exist
    for group in &groups {
        let mut cmd = Command::new("groupadd");
        cmd.arg("-f").arg(group);
        executor.run(&mut cmd)?;
    }

    // Check if user already exists (idempotent)
    let user_exists = if executor.dry_run {
        false // In dry-run, always show what would happen
    } else {
        Command::new("id")
            .arg(username)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };

    if user_exists {
        println!(
            "User {} already exists, ensuring group membership and shell...",
            username
        );
        // Add user to groups if not already a member
        let mut cmd_mod = Command::new("usermod");
        cmd_mod.arg("-aG").arg(groups.join(",")).arg(username);
        executor.run(&mut cmd_mod)?;

        // Ensure shell is zsh
        let mut cmd_chsh = Command::new("chsh");
        cmd_chsh.arg("-s").arg("/bin/zsh").arg(username);
        executor.run(&mut cmd_chsh)?;
    } else {
        let shell = "/bin/zsh";

        let mut cmd_user = Command::new("useradd");
        cmd_user
            .arg("-m")
            .arg("-G")
            .arg(groups.join(","))
            .arg("-s")
            .arg(shell)
            .arg(username);

        executor.run(&mut cmd_user)?;
    }

    // Set user password
    let user_input = format!("{}:{}", username, password);
    let mut cmd_pass = Command::new("chpasswd");
    executor.run_with_input(&mut cmd_pass, &user_input)?;

    Ok(())
}

pub fn configure_sudo(_context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    println!("Configuring sudoers...");
    // Uncomment %wheel ALL=(ALL:ALL) ALL

    if executor.dry_run {
        println!("[DRY RUN] Uncommenting %wheel in /etc/sudoers");
        println!("[DRY RUN] Adding 'Defaults env_reset,pwfeedback' to /etc/sudoers");
    } else {
        let sudoers_path = "/etc/sudoers";
        let content =
            std::fs::read_to_string(sudoers_path).context("Failed to read /etc/sudoers")?;

        let mut new_lines = Vec::new();
        let mut has_pwfeedback = false; // Track if pwfeedback already exists

        for line in content.lines() {
            // Check if pwfeedback line already exists
            if line.contains("Defaults") && line.contains("pwfeedback") {
                has_pwfeedback = true;
            }

            if line.contains("%wheel ALL=(ALL:ALL) ALL") && line.trim().starts_with('#') {
                new_lines.push(line.replacen('#', "", 1).trim().to_string());
            } else {
                new_lines.push(line.to_string());
            }
        }

        // Add defaults only if not present (FIXED: Check has_pwfeedback first)
        if !has_pwfeedback {
            new_lines.push("Defaults env_reset,pwfeedback".to_string());
        }

        std::fs::write(sudoers_path, new_lines.join("\n"))?;
    }

    Ok(())
}

fn configure_plymouth(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    if !context.get_answer_bool(QuestionId::UsePlymouth) {
        return Ok(());
    }

    println!("Configuring Plymouth...");

    if executor.dry_run {
        println!("[DRY RUN] Setting Plymouth theme to spinner");
        println!("[DRY RUN] plymouth-set-default-theme -R spinner");
        return Ok(());
    }

    // Configure Plymouth theme
    let plymouth_conf = "/etc/plymouth/plymouthd.conf";

    // Create Plymouth config directory if it doesn't exist
    std::fs::create_dir_all("/etc/plymouth")?;

    // Note: If encryption is used, the Plymouth theme will not be visible during the
    // password prompt because the theme files are on the encrypted partition.
    let config_content = r#"[Daemon]
Theme=spinner
ShowDelay=0
"#;

    std::fs::write(plymouth_conf, config_content)?;

    // Set the theme and rebuild initramfs
    let mut cmd = Command::new("plymouth-set-default-theme");
    cmd.arg("-R").arg("spinner");
    executor.run(&mut cmd)?;

    Ok(())
}
