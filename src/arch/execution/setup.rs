use super::CommandExecutor;
use anyhow::Result;
use std::process::Command;

pub async fn setup_instantos(executor: &CommandExecutor, username: Option<String>) -> Result<()> {
    println!("Setting up instantOS...");

    setup_instant_repo(executor).await?;
    install_instant_packages(executor)?;

    update_os_release(executor)?;

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
