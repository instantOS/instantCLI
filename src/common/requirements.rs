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

/// Represents a Flatpak application dependency
#[derive(Debug, Clone, Copy)]
pub struct FlatpakPackage {
    pub name: &'static str,
    pub app_id: &'static str,
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
    #[allow(dead_code)]
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

        let installing_msg = format!("Installing {} (package: {})...", self.name, package_name);

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

impl FlatpakPackage {
    pub fn is_installed(&self) -> bool {
        self.tests.iter().any(|test| test.run())
    }

    fn is_flathub_configured() -> bool {
        cmd!("flatpak", "remotes", "--columns=name")
            .read()
            .map(|output| output.lines().any(|line| line.trim() == "flathub"))
            .unwrap_or(false)
    }

    fn setup_flathub() -> Result<()> {
        FzfWrapper::builder()
            .message("Flathub repository needs to be configured to install Flatpak applications.")
            .title("Setting up Flathub")
            .show_message()?;

        cmd!(
            "flatpak",
            "remote-add",
            "--if-not-exists",
            "flathub",
            "https://flathub.org/repo/flathub.flatpakrepo"
        )
        .run()
        .context("Failed to add Flathub remote")?;

        Ok(())
    }

    fn install_from_flathub(&self) -> Result<()> {
        if !Self::is_flathub_configured() {
            Self::setup_flathub()?;
        }

        let installing_msg = format!("Installing {} from Flathub...", self.name);
        FzfWrapper::builder()
            .message(&installing_msg)
            .title("Installing Flatpak")
            .show_message()?;

        cmd!("flatpak", "install", "-y", "flathub", self.app_id)
            .run()
            .with_context(|| format!("Failed to install {} from Flathub", self.name))?;

        if !self.is_installed() {
            return Err(anyhow::anyhow!(
                "Installation of '{}' completed but verification checks still failed",
                self.name
            ));
        }

        let success_msg = format!("Successfully installed {} from Flathub!", self.name);
        FzfWrapper::builder()
            .message(&success_msg)
            .title("Installation Complete")
            .show_message()?;

        Ok(())
    }

    pub fn ensure(&self) -> Result<bool> {
        if self.is_installed() {
            return Ok(true);
        }

        let install_msg = format!(
            "The Flatpak application '{}' is not installed.\n\nDo you want to install it from Flathub?",
            self.name
        );

        let should_install = FzfWrapper::builder()
            .confirm(&install_msg)
            .yes_text("Install")
            .no_text("Cancel")
            .show_confirmation()?;

        let should_install = matches!(should_install, crate::menu_utils::ConfirmResult::Yes);

        if should_install {
            if let Err(err) = self.install_from_flathub() {
                let error_msg = format!(
                    "Failed to install '{}': {}\n\nManual installation:\nflatpak install flathub {}",
                    self.name, err, self.app_id
                );
                FzfWrapper::builder()
                    .message(&error_msg)
                    .title("Installation Failed")
                    .show_message()?;
                return Ok(false);
            }
            Ok(true)
        } else {
            let cancel_msg = format!(
                "The Flatpak application '{}' is required.\n\nManual installation:\nflatpak install flathub {}",
                self.name, self.app_id
            );
            FzfWrapper::builder()
                .message(&cancel_msg)
                .title("Flatpak Required")
                .show_message()?;
            Ok(false)
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

/// Find packages that are not currently installed
fn find_missing_packages(packages: &[RequiredPackage]) -> Vec<&RequiredPackage> {
    packages.iter().filter(|pkg| !pkg.is_installed()).collect()
}

/// Get package names for the detected package manager
fn get_package_names(
    packages: &[&RequiredPackage],
    package_manager: &PackageManager,
) -> Result<Vec<&'static str>> {
    let mut names = Vec::new();
    for pkg in packages {
        let package_name = package_manager.package_name(pkg).ok_or_else(|| {
            anyhow::anyhow!("Package '{}' not available for this system", pkg.name)
        })?;
        names.push(package_name);
    }
    Ok(names)
}

/// Build a message listing packages with their system package names
fn build_package_list_message(
    packages: &[&RequiredPackage],
    package_manager: &PackageManager,
) -> Result<String> {
    let mut msg = String::from("The following packages are required:\n\n");

    for pkg in packages {
        let package_name = package_manager.package_name(pkg).ok_or_else(|| {
            anyhow::anyhow!("Package '{}' not available for this system", pkg.name)
        })?;

        msg.push_str(&format!("  • {} (package: {})\n", pkg.name, package_name));
    }

    Ok(msg)
}

/// Build a message showing failed installations
fn build_failure_message(failed_packages: &[String]) -> String {
    format!(
        "Installation completed but the following packages failed verification:\n\n{}\n\nSome packages may require a system restart or PATH update.",
        failed_packages
            .iter()
            .map(|s| format!("  • {}", s))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

/// Show installation success message
fn show_success_message(count: usize) -> Result<()> {
    let success_msg = format!(
        "Successfully installed {} package{}!",
        count,
        if count == 1 { "" } else { "s" }
    );
    FzfWrapper::builder()
        .message(&success_msg)
        .title("Installation Complete")
        .show_message()
}

/// Show installation cancelled message with hints
fn show_cancelled_message(packages: &[&RequiredPackage]) -> Result<()> {
    let mut cancel_msg = String::from("The following packages are required:\n\n");
    for pkg in packages {
        cancel_msg.push_str(&format!("  • {}\n", pkg.name));
    }
    cancel_msg.push_str(&format!("\n{}", packages[0].install_hint()));

    FzfWrapper::builder()
        .message(&cancel_msg)
        .title("Packages Required")
        .show_message()
}

/// Prompt user for batch installation confirmation
fn prompt_batch_installation(
    packages: &[&RequiredPackage],
    package_manager: &PackageManager,
) -> Result<bool> {
    let mut msg = build_package_list_message(packages, package_manager)?;
    msg.push_str("\nDo you want to install all of them?");

    let should_install = FzfWrapper::builder()
        .confirm(&msg)
        .yes_text("Install All")
        .no_text("Cancel")
        .show_confirmation()?;

    Ok(matches!(
        should_install,
        crate::menu_utils::ConfirmResult::Yes
    ))
}

/// Execute batch installation using the appropriate package manager
fn execute_batch_installation(
    package_names: &[&'static str],
    package_manager: &PackageManager,
) -> Result<()> {
    match package_manager {
        PackageManager::Pacman => {
            let mut args = vec!["pacman", "-S", "--noconfirm"];
            for name in package_names {
                args.push(name);
            }
            cmd("sudo", &args)
                .run()
                .context("Failed to install packages with pacman")?;
        }
        PackageManager::Apt => {
            let mut args = vec!["apt", "install", "-y"];
            for name in package_names {
                args.push(name);
            }
            cmd("sudo", &args)
                .run()
                .context("Failed to install packages with apt")?;
        }
    }
    Ok(())
}

/// Verify that all packages were installed successfully
fn verify_installations(
    packages: &[&RequiredPackage],
    package_manager: &PackageManager,
) -> Result<Vec<String>> {
    let mut failed = Vec::new();
    for pkg in packages {
        if !pkg.is_installed() {
            if let Some(package_name) = package_manager.package_name(pkg) {
                failed.push(format!("{} ({})", pkg.name, package_name));
            } else {
                failed.push(pkg.name.to_string());
            }
        }
    }
    Ok(failed)
}

/// Ensure multiple packages are installed with a single prompt for all missing packages.
/// Returns Ok(true) if all packages are installed or were successfully installed.
/// Returns Ok(false) if user cancelled or any installation failed.
pub fn ensure_packages_batch(packages: &[RequiredPackage]) -> Result<bool> {
    // First, check which packages are missing
    let missing = find_missing_packages(packages);

    if missing.is_empty() {
        return Ok(true);
    }

    // Detect package manager
    let package_manager = PackageManager::detect()
        .ok_or_else(|| anyhow::anyhow!("No supported package manager found"))?;

    // Get package names for installation
    let package_names = get_package_names(&missing, &package_manager)?;

    // Prompt user for installation
    if !prompt_batch_installation(&missing, &package_manager)? {
        show_cancelled_message(&missing)?;
        return Ok(false);
    }

    // Show installation progress message
    let installing_msg = format!(
        "Installing {} package{}...",
        package_names.len(),
        if package_names.len() == 1 { "" } else { "s" }
    );

    FzfWrapper::builder()
        .message(&installing_msg)
        .title("Installing Packages")
        .show_message()?;

    // Install all packages
    execute_batch_installation(&package_names, &package_manager)?;

    // Verify installations
    let failed = verify_installations(&missing, &package_manager)?;

    if !failed.is_empty() {
        let error_msg = build_failure_message(&failed);
        FzfWrapper::builder()
            .message(&error_msg)
            .title("Installation Warning")
            .show_message()?;
        return Ok(false);
    }

    show_success_message(package_names.len())?;
    Ok(true)
}
