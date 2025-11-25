use super::CommandExecutor;
use crate::arch::engine::{InstallContext, QuestionId};
use anyhow::{Context, Result};
use std::process::Command;

pub async fn install_post(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    println!("Running post-installation setup (inside chroot)...");

    setup_instant_repo(executor).await?;
    install_instant_packages(executor)?;
    setup_user_dotfiles(context, executor)?;
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

fn setup_user_dotfiles(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    let username = context
        .get_answer(&QuestionId::Username)
        .context("Username not set")?;

    println!("Setting up dotfiles for user: {}", username);

    // Clone dotfiles
    // su -c "ins dot repo clone https://github.com/instantOS/instantDOTS" username
    let clone_cmd_str = "ins dot repo clone https://github.com/instantOS/instantDOTS";
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

    // systemctl enable lightdm
    let mut cmd = Command::new("systemctl");
    cmd.arg("enable").arg("lightdm");

    executor.run(&mut cmd)?;

    Ok(())
}
