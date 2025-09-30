use crate::common::git;
use crate::common::progress::create_spinner;
use crate::dev::fuzzy::select_package;
use crate::dev::package::Package;
use crate::ui::prelude::*;
use anyhow::{Context, Result};
use duct::cmd;
use git2::Repository;
use std::path::PathBuf;

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
            let mut repo =
                Repository::open(&self.path).context("Failed to open package repository")?;

            // Check if there are local changes by examining the repository status
            let has_local_changes = self.has_local_changes(&repo)?;

            if has_local_changes {
                self.handle_local_changes()?;
            }

            // Pull latest changes
            git::clean_and_pull(&mut repo).context("Failed to pull latest changes")?;
        } else {
            // Clone repository
            git::clone_repo(&self.url, &self.path, Some("main"), Some(3))
                .context("Failed to clone repository")?;
        }

        Ok(())
    }

    fn has_local_changes(&self, repo: &Repository) -> Result<bool> {
        // Check if there are uncommitted changes
        let statuses = repo
            .statuses(None)
            .context("Failed to get repository status")?;

        Ok(!statuses.is_empty())
    }

    fn handle_local_changes(&self) -> Result<()> {
        emit(
            Level::Warn,
            "dev.install.local_changes",
            "⚠️ Local changes detected in package repository",
            None
        );
        emit(
            Level::Info,
            "dev.install.stash",
            "🔄 Stashing local changes...",
            None
        );

        let mut repo =
            Repository::open(&self.path).context("Failed to open repository for stashing")?;

        // Use git2 to stash changes
        let signature = repo.signature().context("Failed to get git signature")?;

        repo.stash_save(&signature, "Auto-stash by instantCLI", None)
            .context("Failed to stash changes")?;

        Ok(())
    }
}

pub fn build_and_install_package(package: &Package, debug: bool) -> Result<()> {
    if debug {
        emit(
            Level::Debug,
            "dev.install.build.start",
            format!("🔧 Building package: {}", package.name).as_str(),
            None
        );
    }

    emit(
        Level::Info,
        "dev.install.build.install",
        format!("📦 Building and installing {}... (This may be interactive)", package.name).as_str(),
        None
    );

    // Build and install package (interactive - no spinner)
    cmd!("makepkg", "-si")
        .dir(&package.path)
        .run()
        .context("Failed to build and install package")?;

    emit(
        Level::Success,
        "dev.install.success",
        format!("🎉 Successfully installed {}", package.name).as_str(),
        None
    );

    Ok(())
}

pub async fn handle_install(debug: bool) -> Result<()> {
    if debug {
        emit(
            Level::Debug,
            "dev.install.start",
            "🚀 Starting package installation...",
            None
        );
    }

    let pb = create_spinner("Preparing package repository...".to_string());

    // Initialize and update repository
    let repo = PackageRepo::new()?;
    repo.ensure_updated()?;

    pb.finish_with_message("Package repository ready".to_string());

    if debug {
        emit(
            Level::Debug,
            "dev.install.discover",
            "🔍 Discovering packages...",
            None
        );
    }

    // Discover available packages
    let packages = Package::discover_packages(&repo.path).context("Failed to discover packages")?;

    if packages.is_empty() {
        return Err(anyhow::anyhow!("No packages found in repository"));
    }

    if debug {
        emit(
            Level::Debug,
            "dev.install.packages.count",
            format!("📊 Found {} packages", packages.len()).as_str(),
            None
        );
        for pkg in &packages {
            emit(
                Level::Debug,
                "dev.install.packages.item",
                format!("📋  - {} ({:?})", pkg.name, pkg.description).as_str(),
                None
            );
        }
    }

    // Select package
    let selected_package = select_package(packages).context("Failed to select package")?;

    if debug {
        emit(
            Level::Debug,
            "dev.install.selected",
            format!("📝 Selected package: {}", selected_package.name).as_str(),
            None
        );
    }

    // Build and install package
    build_and_install_package(&selected_package, debug)?;

    Ok(())
}
