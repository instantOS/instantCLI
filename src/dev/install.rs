use crate::common::create_spinner;
use crate::dev::fuzzy::select_package;
use crate::dev::package::Package;
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;

pub struct PackageRepo {
    pub path: PathBuf,
    pub url: String,
}

impl PackageRepo {
    pub fn new() -> Result<Self> {
        let cache_dir = dirs::cache_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not find cache directory"))?
            .join("instantos");

        std::fs::create_dir_all(&cache_dir).context("Failed to create cache directory")?;

        let repo_path = cache_dir.join("extra");
        let url = "https://github.com/instantOS/extra".to_string();

        Ok(PackageRepo {
            path: repo_path,
            url,
        })
    }

    pub fn ensure_updated(&self) -> Result<()> {
        if self.path.exists() {
            // Repository exists, pull latest changes
            std::env::set_current_dir(&self.path)
                .context("Failed to change directory")?;

            // Check if there are local changes
            let output = Command::new("git")
                .args(&["status", "--porcelain"])
                .output()
                .context("Failed to check git status")?;

            let has_local_changes = output.status.success() && !output.stdout.is_empty();

            if has_local_changes {
                self.handle_local_changes()?;
            }

            // Pull latest changes
            let status = Command::new("git")
                .args(&["pull", "origin", "main", "--depth=3"])
                .status()
                .context("Failed to pull latest changes")?;

            if !status.success() {
                return Err(anyhow::anyhow!("Failed to pull latest changes"));
            }
        } else {
            // Clone repository
            let parent_dir = self.path.parent().unwrap();
            std::env::set_current_dir(parent_dir)
                .context("Failed to change directory")?;

            let status = Command::new("git")
                .args(&["clone", &self.url, "--depth=3"])
                .status()
                .context("Failed to clone repository")?;

            if !status.success() {
                return Err(anyhow::anyhow!("Failed to clone repository"));
            }
        }

        Ok(())
    }

    fn handle_local_changes(&self) -> Result<()> {
        // Check if instantwm is running
        let instantwm_running = Command::new("pgrep")
            .arg("instantwm")
            .status()
            .map(|status| status.success())
            .unwrap_or(false);

        if instantwm_running {
            eprintln!("⚠️  Local changes detected and instantwm is running");
            eprintln!("💾 Stashing local changes...");
        } else {
            eprintln!("⚠️  Local changes detected in package repository");
            eprintln!("💾 Stashing local changes...");
        }

        let status = Command::new("git")
            .arg("stash")
            .status()
            .context("Failed to stash changes")?;

        if !status.success() {
            return Err(anyhow::anyhow!("Failed to stash changes"));
        }

        Ok(())
    }
}

pub fn build_and_install_package(package: &Package, debug: bool) -> Result<()> {
    if debug {
        eprintln!("🔍 Building package: {}", package.name);
    }

    let pb = create_spinner(format!("Building and installing {}...", package.name));

    // Change to package directory
    std::env::set_current_dir(&package.path)
        .context("Failed to change directory")?;

    // Build and install package
    let status = Command::new("makepkg")
        .arg("-si")
        .status()
        .context("Failed to build and install package")?;

    if !status.success() {
        return Err(anyhow::anyhow!("Failed to build and install package"));
    }

    pb.finish_with_message(format!("✅ Successfully installed {}", package.name));

    Ok(())
}

pub async fn handle_install(debug: bool) -> Result<()> {
    if debug {
        eprintln!("🔍 Starting package installation...");
    }

    let pb = create_spinner("Preparing package repository...".to_string());

    // Initialize and update repository
    let repo = PackageRepo::new()?;
    repo.ensure_updated()?;

    pb.finish_with_message("Package repository ready".to_string());

    if debug {
        eprintln!("📦 Discovering packages...");
    }

    // Discover available packages
    let packages = Package::discover_packages(&repo.path).context("Failed to discover packages")?;

    if packages.is_empty() {
        return Err(anyhow::anyhow!("No packages found in repository"));
    }

    if debug {
        eprintln!("📋 Found {} packages:", packages.len());
        for pkg in &packages {
            eprintln!("  - {} ({:?})", pkg.name, pkg.description);
        }
    }

    // Select package
    let selected_package = select_package(packages).context("Failed to select package")?;

    if debug {
        eprintln!("🎯 Selected package: {}", selected_package.name);
    }

    // Build and install package
    build_and_install_package(&selected_package, debug)?;

    Ok(())
}
