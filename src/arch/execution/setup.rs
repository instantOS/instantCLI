use super::CommandExecutor;
use anyhow::Result;
use std::process::Command;

use crate::arch::engine::{InstallContext, QuestionId};

pub async fn setup_instantos(
    context: &InstallContext,
    executor: &CommandExecutor,
    override_user: Option<String>,
) -> Result<()> {
    println!("Setting up instantOS...");

    setup_instant_repo(executor).await?;

    // Install extended packages (GUI, tools, drivers)
    install_packages(context, executor)?;

    update_os_release(executor)?;

    // Determine username: override > context > SUDO_USER
    let username = override_user.or_else(|| context.get_answer(&QuestionId::Username).cloned());

    if let Some(user) = username {
        setup_user_dotfiles(&user, executor)?;
    } else {
        println!("Skipping dotfiles setup: No user specified and SUDO_USER not found.");
    }

    enable_services(executor, context)?;

    Ok(())
}

async fn setup_instant_repo(executor: &CommandExecutor) -> Result<()> {
    println!("Setting up instantOS repository...");
    crate::common::pacman::setup_instant_repo(executor.dry_run).await?;

    // Update repositories
    println!("Updating repositories...");
    let mut cmd = Command::new("pacman");
    cmd.arg("-Sy");
    executor.run(&mut cmd)?;

    Ok(())
}

fn install_packages(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    println!("Installing extended packages...");

    let mut packages = vec![
        "sway",
        "openssh",
        "mesa",
        "xorg-xwayland",
        "polkit",
        // instantOS packages
        "instantdepend",
        "instantos",
        "instantextra",
        "lightdm",
        "lightdm-gtk-greeter",
    ];

    // GPU packages
    for gpu in &context.system_info.gpus {
        match gpu {
            crate::arch::engine::GpuKind::Nvidia => {
                println!("Detected NVIDIA GPU, adding nvidia");
                packages.push("nvidia");
                packages.push("nvidia-utils");
                packages.push("nvidia-settings");
            }
            crate::arch::engine::GpuKind::Amd => {
                println!("Detected AMD GPU, adding vulkan support");
                packages.push("vulkan-radeon");
                packages.push("lib32-vulkan-radeon");
                // Optional AMD GPU packages for better support
                packages.push("libva-mesa-driver");
                packages.push("lib32-libva-mesa-driver");
            }
            crate::arch::engine::GpuKind::Intel => {
                println!("Detected Intel GPU, adding vulkan support");
                packages.push("vulkan-intel");
                packages.push("lib32-vulkan-intel");
                // Intel media driver for video acceleration
                packages.push("intel-media-driver");
            }
            crate::arch::engine::GpuKind::Other(name) => {
                println!("Detected unknown GPU: {}, adding basic mesa support", name);
                packages.push("mesa");
                packages.push("lib32-mesa");
            }
        }
    }

    // VM Guest Tools
    if let Some(vm_type) = &context.system_info.vm_type {
        println!("Detected VM: {}, adding guest tools", vm_type);
        match vm_type.as_str() {
            "kvm" | "qemu" | "bochs" => {
                packages.push("qemu-guest-agent");
            }
            "vmware" => {
                packages.push("open-vm-tools");
            }
            "oracle" => {
                packages.push("virtualbox-guest-utils");
            }
            _ => {
                println!("No specific guest tools for VM type: {}", vm_type);
            }
        }
    }

    // Plymouth support
    if context.get_answer_bool(QuestionId::UsePlymouth) {
        println!("Plymouth enabled, adding plymouth package");
        packages.push("plymouth");
    }

    super::pacman::install(&packages, executor)?;

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
        // su -c "ins dot repo clone https://github.com/instantOS/dotfiles" username
        // TODO: make repo url a constant
        let clone_cmd_str = "ins dot repo clone https://github.com/instantOS/dotfiles";
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

    if !other_dm_enabled {
        services.push("lightdm");
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
