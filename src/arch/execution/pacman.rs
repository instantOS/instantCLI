use anyhow::{Context, Result};
use rand::seq::SliceRandom;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

use super::CommandExecutor;

fn shuffle_mirrors() -> Result<()> {
    let path = Path::new("/etc/pacman.d/mirrorlist");
    if !path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(path).context("Failed to read mirrorlist")?;
    let mut new_lines = Vec::new();
    let mut server_pool = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Server") && trimmed.contains('=') {
            server_pool.push(line.to_string());
        } else {
            if !server_pool.is_empty() {
                let mut rng = rand::thread_rng();
                server_pool.shuffle(&mut rng);
                new_lines.append(&mut server_pool);
            }
            new_lines.push(line.to_string());
        }
    }
    if !server_pool.is_empty() {
        let mut rng = rand::thread_rng();
        server_pool.shuffle(&mut rng);
        new_lines.append(&mut server_pool);
    }

    let output = new_lines.join("\n");
    std::fs::write(path, output + "\n").context("Failed to write mirrorlist")?;

    println!("Shuffled mirrors in /etc/pacman.d/mirrorlist");
    Ok(())
}

/// Installs packages using pacman with a retry mechanism similar to `pacloop`
pub fn install(packages: &[&str], executor: &CommandExecutor) -> Result<()> {
    if packages.is_empty() {
        return Ok(());
    }

    let mut attempt = 0;
    // We use a file to track if we've refreshed the keyring, similar to the bash script
    // This persists across retries within the same session if the file remains.
    let keyring_refreshed_path = Path::new("/tmp/instant_arch_keyring_refreshed");

    loop {
        attempt += 1;
        if attempt > 10 {
            anyhow::bail!(
                "Package installation failed after 10 attempts. Please check your internet connection."
            );
        }
        if attempt > 1 {
            println!(
                "Retry attempt {}/10 for packages: {}",
                attempt,
                packages.join(" ")
            );
        } else {
            println!("Installing packages: {}", packages.join(" "));
        }

        let mut cmd = Command::new("pacman");
        cmd.arg("-S")
            .arg("--noconfirm")
            .arg("--needed")
            .args(packages);

        match executor.run(&mut cmd) {
            Ok(_) => {
                println!("Successfully installed packages.");
                return Ok(());
            }
            Err(e) => {
                println!("Package installation failed: {}", e);
                println!("Ensure you are connected to the internet.");

                // Check if we should refresh keyring
                // Don't refresh if we are currently trying to install the keyring itself
                let installing_keyring = packages.contains(&"archlinux-keyring");
                let keyring_already_refreshed = keyring_refreshed_path.exists();

                if !keyring_already_refreshed && !installing_keyring {
                    println!("Attempting to refresh archlinux-keyring...");
                    let mut key_cmd = Command::new("pacman");
                    key_cmd.args(["-Sy", "archlinux-keyring", "--noconfirm"]);

                    if let Err(e) = executor.run(&mut key_cmd) {
                        println!("Warning: Failed to refresh keyring: {}", e);
                    } else {
                        // Mark as refreshed
                        if let Err(e) = std::fs::File::create(keyring_refreshed_path) {
                            println!("Warning: Failed to create lock file: {}", e);
                        }
                        // Continue immediately after keyring refresh to try original packages again
                        continue;
                    }
                }

                // Update mirrors
                println!("Updating mirrors...");
                if which::which("reflector").is_ok() {
                    let mut ref_cmd = Command::new("reflector");
                    ref_cmd.args([
                        "--latest",
                        "40",
                        "--protocol",
                        "http,https",
                        "--sort",
                        "rate",
                        "--save",
                        "/etc/pacman.d/mirrorlist",
                    ]);
                    if let Err(e) = executor.run(&mut ref_cmd) {
                        println!("Warning: Reflector failed: {}", e);
                    }
                } else {
                    // Fallback or other mirror tools?
                    // The bash script used pacman-mirrors (Manjaro specific usually)
                    // We'll stick to reflector or just skip if not present.
                    println!("Reflector not found, skipping mirror optimization.");
                }

                if let Err(e) = shuffle_mirrors() {
                    println!("Warning: Failed to shuffle mirrors: {}", e);
                }

                // Update repos
                println!("Updating repositories...");
                let mut up_cmd = Command::new("pacman");
                up_cmd.arg("-Sy");
                if let Err(e) = executor.run(&mut up_cmd) {
                    println!("Warning: Repo update failed: {}", e);
                }

                println!("Retrying package installation in 4 seconds...");
                thread::sleep(Duration::from_secs(4));
            }
        }
    }
}

/// Wrapper for pacstrap with retry logic
pub fn pacstrap(mount_point: &str, packages: &[&str], executor: &CommandExecutor) -> Result<()> {
    if packages.is_empty() {
        return Ok(());
    }

    let mut attempt = 0;

    loop {
        attempt += 1;
        if attempt > 10 {
            anyhow::bail!(
                "Pacstrap failed after 10 attempts. Please check your internet connection."
            );
        }
        if attempt > 1 {
            println!("Retry attempt {}/10 for pacstrap", attempt);
        }

        let mut cmd = Command::new("pacstrap");
        cmd.arg(mount_point);
        cmd.args(packages);

        match executor.run(&mut cmd) {
            Ok(_) => {
                // Clean up cache
                let mut clean_cmd = Command::new("pacman");
                clean_cmd.args(["-Scc", "--noconfirm"]);
                // We pipe "yes" to it effectively by using noconfirm, but pacman -Scc asks twice usually.
                // The bash script used `yes | pacman -Scc`.
                // `pacman -Scc --noconfirm` still asks for confirmation in some versions?
                // Let's try to run it with input "y\ny\n" just in case.
                if let Err(e) = executor.run_with_input(&mut clean_cmd, "y\ny\n") {
                    println!("Warning: Failed to clean cache: {}", e);
                }
                return Ok(());
            }
            Err(e) => {
                println!("Pacstrap failed: {}", e);
                println!("Ensure you are connected to the internet.");

                if let Err(e) = shuffle_mirrors() {
                    println!("Warning: Failed to shuffle mirrors: {}", e);
                }

                println!("Retrying in 2 seconds...");
                thread::sleep(Duration::from_secs(2));
            }
        }
    }
}
