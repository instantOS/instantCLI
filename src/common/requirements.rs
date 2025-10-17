use std::path::Path;

use crate::menu_utils::FzfWrapper;
use anyhow::{Context, Result};
use duct::cmd;

/// Tests for determining whether a dependency is available on the system.
#[derive(Debug, Clone, Copy)]
pub enum InstallTest {
    /// Succeeds when `which <program>` resolves.
    WhichSucceeds(&'static str),
    /// Succeeds when the given path exists.
    FileExists(&'static str),
    /// Succeeds when the command exits with status 0.
    CommandSucceeds {
        program: &'static str,
        args: &'static [&'static str],
    },
}

impl InstallTest {
    pub fn run(self) -> bool {
        match self {
            InstallTest::WhichSucceeds(program) => which::which(program).is_ok(),
            InstallTest::FileExists(path) => Path::new(path).exists(),
            InstallTest::CommandSucceeds { program, args } => cmd(program, args).run().is_ok(),
        }
    }
}

/// Represents an external dependency a setting may require.
#[derive(Debug, Clone, Copy)]
pub struct RequiredPackage {
    pub name: &'static str,
    pub arch_package_name: Option<&'static str>,
    pub ubuntu_package_name: Option<&'static str>,
    pub tests: &'static [InstallTest],
}

#[derive(Debug, Clone)]
pub enum PackageManager {
    Pacman,
    Apt,
}

impl PackageManager {
    /// Detect the available package manager on the system
    pub fn detect() -> Option<Self> {
        if which::which("pacman").is_ok() {
            Some(PackageManager::Pacman)
        } else if which::which("apt").is_ok() {
            Some(PackageManager::Apt)
        } else {
            None
        }
    }

    /// Get the package name for this package manager
    pub fn package_name(&self, package: &RequiredPackage) -> Option<&'static str> {
        match self {
            PackageManager::Pacman => package.arch_package_name,
            PackageManager::Apt => package.ubuntu_package_name,
        }
    }

    /// Install a package using this package manager
    pub fn install_package(&self, package: &RequiredPackage) -> Result<()> {
        let package_name = self
            .package_name(package)
            .ok_or_else(|| anyhow::anyhow!("No package name available for this package manager"))?;

        match self {
            PackageManager::Pacman => {
                cmd!("sudo", "pacman", "-S", package_name)
                    .run()
                    .with_context(|| format!("Failed to install {} with pacman", package.name))?;
            }
            PackageManager::Apt => {
                cmd!("sudo", "apt", "install", package_name)
                    .run()
                    .with_context(|| format!("Failed to install {} with apt", package.name))?;
            }
        }
        Ok(())
    }
}

impl RequiredPackage {
    pub fn is_installed(&self) -> bool {
        self.tests.iter().any(|test| test.run())
    }

    /// Ensure the package is installed, prompting for installation if needed
    pub fn ensure(&self) -> Result<bool> {
        if self.is_installed() {
            return Ok(true);
        }

        // Package is not installed, prompt for installation
        let install_msg = format!(
            "The required package '{}' is not installed.\n\nDo you want to install it?",
            self.name
        );

        let should_install = FzfWrapper::builder()
            .confirm(&install_msg)
            .yes_text("Install")
            .no_text("Cancel")
            .show_confirmation()?;

        let should_install = matches!(should_install, crate::menu_utils::ConfirmResult::Yes);

        if should_install {
            if let Err(err) = self.install_with_prompt() {
                // Installation failed, show message and return false
                let error_msg = format!(
                    "Failed to install '{}': {}\n\nThis package is required for the selected setting.\n\n{}",
                    self.name,
                    err,
                    self.install_hint()
                );
                FzfWrapper::builder()
                    .message(&error_msg)
                    .title("Installation Failed")
                    .show_message()?;
                return Ok(false);
            }
            Ok(true)
        } else {
            // User declined installation
            let cancel_msg = format!(
                "The package '{}' is required for this setting.\n\n{}",
                self.name,
                self.install_hint()
            );
            FzfWrapper::builder()
                .message(&cancel_msg)
                .title("Package Required")
                .show_message()?;
            Ok(false)
        }
    }

    /// Get the package name for the current system
    pub fn get_package_name(&self) -> Option<(&'static str, PackageManager)> {
        let pm = PackageManager::detect()?;
        let name = pm.package_name(self)?;
        Some((name, pm))
    }

    /// Attempt to install the package
    fn install_with_prompt(&self) -> Result<()> {
        let package_manager = PackageManager::detect()
            .ok_or_else(|| anyhow::anyhow!("No supported package manager found"))?;

        let package_name = package_manager
            .package_name(self)
            .ok_or_else(|| anyhow::anyhow!("Package not available for this system"))?;

        let installing_msg = format!(
            "Installing {} (package: {})...",
            self.name,
            package_name
        );

        // Show installation progress message
        FzfWrapper::builder()
            .message(&installing_msg)
            .title("Installing Package")
            .show_message()?;

        package_manager.install_package(self)?;

        // Verify installation succeeded
        if !self.is_installed() {
            return Err(anyhow::anyhow!(
                "Installation of package '{}' completed but verification checks still failed",
                package_name
            ));
        }

        let success_msg = format!("Successfully installed {} ({})!", self.name, package_name);
        FzfWrapper::builder()
            .message(&success_msg)
            .title("Installation Complete")
            .show_message()?;

        Ok(())
    }

    pub fn install_hint(&self) -> String {
        let mut hints = Vec::new();
        if let Some(pkg) = self.arch_package_name {
            hints.push(format!("pacman -S {pkg}"));
        }
        if let Some(pkg) = self.ubuntu_package_name {
            hints.push(format!("apt install {pkg}"));
        }
        if hints.is_empty() {
            format!("Install `{}`", self.name)
        } else {
            format!("Try one of: {}", hints.join(" | "))
        }
    }
}

/// Required package for restic (backup tool used by game commands)
pub static RESTIC_PACKAGE: RequiredPackage = RequiredPackage {
    name: "restic",
    arch_package_name: Some("restic"),
    ubuntu_package_name: Some("restic"),
    tests: &[InstallTest::WhichSucceeds("restic")],
};

/// Ensure multiple packages are installed with a single prompt for all missing packages.
/// Returns Ok(true) if all packages are installed or were successfully installed.
/// Returns Ok(false) if user cancelled or any installation failed.
pub fn ensure_packages_batch(packages: &[RequiredPackage]) -> Result<bool> {
    // First, check which packages are missing
    let missing: Vec<&RequiredPackage> = packages
        .iter()
        .filter(|pkg| !pkg.is_installed())
        .collect();

    if missing.is_empty() {
        return Ok(true);
    }

    // Detect package manager
    let package_manager = PackageManager::detect()
        .ok_or_else(|| anyhow::anyhow!("No supported package manager found"))?;

    // Build the prompt message with both human-readable names and package names
    let mut msg = String::from("The following packages are required:\n\n");
    let mut package_names = Vec::new();
    
    for pkg in &missing {
        let package_name = package_manager
            .package_name(pkg)
            .ok_or_else(|| anyhow::anyhow!("Package '{}' not available for this system", pkg.name))?;
        
        msg.push_str(&format!("  • {} (package: {})\n", pkg.name, package_name));
        package_names.push(package_name);
    }
    
    msg.push_str("\nDo you want to install all of them?");

    // Show single confirmation for all packages
    let should_install = FzfWrapper::builder()
        .confirm(&msg)
        .yes_text("Install All")
        .no_text("Cancel")
        .show_confirmation()?;

    if !matches!(should_install, crate::menu_utils::ConfirmResult::Yes) {
        // User declined, show what they need to install manually
        let mut cancel_msg = String::from("The following packages are required:\n\n");
        for pkg in &missing {
            cancel_msg.push_str(&format!("  • {}\n", pkg.name));
        }
        cancel_msg.push_str(&format!("\n{}", missing[0].install_hint()));
        
        FzfWrapper::builder()
            .message(&cancel_msg)
            .title("Packages Required")
            .show_message()?;
        return Ok(false);
    }

    // Install all packages in a single command for better performance
    let installing_msg = format!(
        "Installing {} package{}...",
        package_names.len(),
        if package_names.len() == 1 { "" } else { "s" }
    );
    
    FzfWrapper::builder()
        .message(&installing_msg)
        .title("Installing Packages")
        .show_message()?;

    // Install all at once
    match package_manager {
        PackageManager::Pacman => {
            let mut args = vec!["pacman", "-S", "--noconfirm"];
            for name in &package_names {
                args.push(name);
            }
            cmd("sudo", &args)
                .run()
                .context("Failed to install packages with pacman")?;
        }
        PackageManager::Apt => {
            let mut args = vec!["apt", "install", "-y"];
            for name in &package_names {
                args.push(name);
            }
            cmd("sudo", &args)
                .run()
                .context("Failed to install packages with apt")?;
        }
    }

    // Verify all installations succeeded
    let mut failed = Vec::new();
    for pkg in &missing {
        if !pkg.is_installed() {
            if let Some(package_name) = package_manager.package_name(pkg) {
                failed.push(format!("{} ({})", pkg.name, package_name));
            } else {
                failed.push(pkg.name.to_string());
            }
        }
    }

    if !failed.is_empty() {
        let error_msg = format!(
            "Installation completed but the following packages failed verification:\n\n{}\n\nSome packages may require a system restart or PATH update.",
            failed.iter().map(|s| format!("  • {}", s)).collect::<Vec<_>>().join("\n")
        );
        FzfWrapper::builder()
            .message(&error_msg)
            .title("Installation Warning")
            .show_message()?;
        return Ok(false);
    }

    let success_msg = format!(
        "Successfully installed {} package{}!",
        package_names.len(),
        if package_names.len() == 1 { "" } else { "s" }
    );
    FzfWrapper::builder()
        .message(&success_msg)
        .title("Installation Complete")
        .show_message()?;

    Ok(true)
}

