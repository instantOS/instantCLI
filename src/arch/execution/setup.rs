use super::CommandExecutor;
use anyhow::{Context, Result};
use std::process::Command;

pub async fn setup_instantos(executor: &CommandExecutor, username: Option<String>) -> Result<()> {
    println!("Setting up instantOS...");

    setup_instant_repo(executor).await?;
    install_instant_packages(executor)?;

    if let Some(user) = username {
        setup_user_dotfiles(&user, executor)?;
    } else {
        println!("Skipping dotfiles setup: No user specified and SUDO_USER not found.");
    }

    enable_services(executor)?;

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

fn install_instant_packages(executor: &CommandExecutor) -> Result<()> {
    println!("Installing instantOS packages...");

    let packages = vec!["instantdepend", "instantos", "instantextra"];

    let mut cmd = Command::new("pacman");
    cmd.arg("-S")
        .arg("--noconfirm")
        .arg("--needed")
        .args(&packages);

    executor.run(&mut cmd)?;

    Ok(())
}

fn setup_user_dotfiles(username: &str, executor: &CommandExecutor) -> Result<()> {
    println!("Setting up dotfiles for user: {}", username);

    // Clone dotfiles
    // su -c "ins dot repo clone https://github.com/instantOS/dotfiles" username
    // TODO: make repo url a constant
    let clone_cmd_str = "ins dot repo clone https://github.com/instantOS/dotfiles";
    let mut cmd_clone = Command::new("su");
    cmd_clone.arg("-c").arg(clone_cmd_str).arg(username);

    executor.run(&mut cmd_clone)?;

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

fn enable_services(executor: &CommandExecutor) -> Result<()> {
    println!("Enabling services...");

    let mut services = vec!["NetworkManager", "sshd"];

    // Check if other display managers are enabled
    // We check this directly via Command because CommandExecutor errors on failure (non-zero exit),
    // and systemctl is-enabled returns non-zero if disabled.
    let mut other_dm_enabled = false;

    for dm in &["sddm", "gdm"] {
        let mut cmd = Command::new("systemctl");
        cmd.arg("is-enabled").arg(dm);
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());

        if let Ok(status) = cmd.status() {
            if status.success() {
                println!("Detected enabled display manager: {}", dm);
                other_dm_enabled = true;
                break;
            }
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
