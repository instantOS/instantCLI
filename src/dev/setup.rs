use crate::common::distro;
use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::ui::prelude::*;
use anyhow::{Context, Result};
use colored::Colorize;
use std::process::Command;

pub async fn handle_setup(debug: bool) -> Result<()> {
    if !distro::is_live_iso() {
        emit(
            Level::Error,
            "dev.setup.not_live",
            "Not running in a live ISO environment. Aborting to prevent system damage.",
            None,
        );
        return Err(anyhow::anyhow!(
            "This command can only be run in a live ISO environment"
        ));
    } else {
        emit(
            Level::Info,
            "dev.setup.live_detected",
            "Live ISO environment detected",
            None,
        );
    }

    // 1. Install packages: zsh, git, mise, neovim
    install_packages(debug)?;

    // 2. Clone and apply instantOS dotfiles
    setup_dotfiles(debug)?;

    // 3. Install opencode
    install_opencode(debug)?;

    emit(
        Level::Success,
        "dev.setup.complete",
        "Development environment setup complete!",
        None,
    );

    Ok(())
}

fn install_packages(debug: bool) -> Result<()> {
    let packages = ["zsh", "git", "mise", "neovim"];

    emit(
        Level::Info,
        "dev.setup.install",
        &format!("Installing packages: {}", packages.join(", ")),
        None,
    );

    let mut cmd = Command::new("pacman");
    cmd.arg("-Sy").arg("--noconfirm").args(&packages);

    if debug {
        println!("Running: {:?}", cmd);
    }

    let status = cmd.status().context("Failed to execute pacman")?;

    if !status.success() {
        return Err(anyhow::anyhow!("Failed to install packages"));
    }

    Ok(())
}

fn setup_dotfiles(debug: bool) -> Result<()> {
    emit(
        Level::Info,
        "dev.setup.dotfiles",
        "Setting up instantOS dotfiles...",
        None,
    );

    // Initialize Config and Database
    // We can use the default config path
    let mut config = Config::load(None)?;
    config.ensure_directories()?;
    let db = Database::new(config.database_path().to_path_buf())?;

    // Clone the repo
    let repo_url = "https://github.com/instantOS/dotfiles";
    let repo_name = "dotfiles"; // Default name

    // Check if already exists
    if config.repos.iter().any(|r| r.name == repo_name) {
        emit(
            Level::Info,
            "dev.setup.dotfiles.exists",
            "Dotfiles repository already configured",
            None,
        );
    } else {
        crate::dot::repo::commands::clone_repository(
            &mut config,
            &db,
            repo_url,
            Some(repo_name),
            None, // Default branch
            debug,
        )?;
    }

    // Apply is handled by clone_repository automatically if successful,
    // but if it already existed, we might want to ensure it's applied?
    // clone_repository in repo/commands.rs calls apply_all_repos.
    // If we skipped cloning, we should probably apply.

    if config.repos.iter().any(|r| r.name == repo_name) {
        crate::dot::apply_all(&config, &db)?;
    }

    Ok(())
}

fn install_opencode(debug: bool) -> Result<()> {
    emit(
        Level::Info,
        "dev.setup.opencode",
        "Installing OpenCode...",
        None,
    );

    let install_script = "curl -fsSL https://opencode.ai/install | bash";

    let mut cmd = Command::new("bash");
    cmd.arg("-c").arg(install_script);

    if debug {
        println!("Running: {:?}", cmd);
    }

    let status = cmd
        .status()
        .context("Failed to execute opencode install script")?;

    if !status.success() {
        return Err(anyhow::anyhow!("Failed to install OpenCode"));
    }

    Ok(())
}
