use crate::common::create_spinner;
use crate::dev::fuzzy::select_package;
use crate::dev::package::Package;
use anyhow::{Context, Result};
use std::path::PathBuf;
use xshell::Shell;

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
        let sh = Shell::new()?;

        if self.path.exists() {
            // Repository exists, pull latest changes
            sh.change_dir(&self.path);

            // Check if there are local changes
            let has_local_changes = sh.cmd("git status --porcelain").read().is_ok()
                && !sh.cmd("git status --porcelain").read()?.is_empty();

            if has_local_changes {
                self.handle_local_changes(&sh)?;
            }

            // Pull latest changes
            sh.cmd("git pull")
                .arg("origin")
                .arg("main")
                .arg("--depth=3")
                .run()
                .context("Failed to pull latest changes")?;
        } else {
            // Clone repository
            let parent_dir = self.path.parent().unwrap();
            sh.change_dir(parent_dir);

            sh.cmd("git clone")
                .arg(&self.url)
                .arg("--depth=3")
                .run()
                .context("Failed to clone repository")?;
        }

        Ok(())
    }

    fn handle_local_changes(&self, sh: &Shell) -> Result<()> {
        // Check if instantwm is running
        let instantwm_running = sh.cmd("pgrep instantwm").ignore_status().run().is_ok();

        if instantwm_running {
            eprintln!("⚠️  Local changes detected and instantwm is running");
            eprintln!("💾 Stashing local changes...");
            sh.cmd("git stash")
                .run()
                .context("Failed to stash changes")?;
        } else {
            eprintln!("⚠️  Local changes detected in package repository");
            eprintln!("💾 Stashing local changes...");
            sh.cmd("git stash")
                .run()
                .context("Failed to stash changes")?;
        }
        Ok(())
    }
}

pub fn build_and_install_package(package: &Package, debug: bool) -> Result<()> {
    let sh = Shell::new()?;

    if debug {
        eprintln!("🔍 Building package: {}", package.name);
    }

    let pb = create_spinner(format!("Building and installing {}...", package.name));

    // Change to package directory
    sh.change_dir(&package.path);

    // Build and install package
    sh.cmd("makepkg")
        .arg("-si")
        .run()
        .context("Failed to build and install package")?;

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
