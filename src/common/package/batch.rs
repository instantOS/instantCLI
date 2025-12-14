//! Batched package installation.
//!
//! This module provides functionality to batch multiple package installations
//! by package manager, reducing the number of prompts and sudo invocations.

use std::collections::HashMap;

use anyhow::{Context, Result};

use super::{Dependency, PackageDefinition, PackageManager};
use crate::common::requirements::PackageStatus;
use crate::menu_utils::FzfWrapper;

/// A batch of packages to install, grouped by package manager.
#[derive(Debug, Default)]
pub struct InstallBatch {
    /// Packages grouped by manager
    batches: HashMap<PackageManager, Vec<PackageToInstall>>,
    /// Dependencies that couldn't be resolved
    unresolved: Vec<&'static str>,
}

/// Information about a package to install
#[derive(Debug)]
pub(crate) struct PackageToInstall {
    /// Human-readable name of the dependency
    pub dependency_name: &'static str,
    /// The package definition to install
    pub package_def: &'static PackageDefinition,
}

impl InstallBatch {
    /// Create a new empty install batch.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a dependency to the batch.
    ///
    /// Returns `Ok(true)` if the dependency was added to the batch.
    /// Returns `Ok(false)` if the dependency is already installed.
    /// Returns `Err` if no suitable package could be found.
    pub fn add(&mut self, dep: &'static Dependency) -> Result<bool> {
        if dep.is_installed() {
            return Ok(false); // Already installed
        }

        if let Some(pkg) = dep.get_best_package() {
            self.batches
                .entry(pkg.manager)
                .or_default()
                .push(PackageToInstall {
                    dependency_name: dep.name,
                    package_def: pkg,
                });
            Ok(true)
        } else {
            // No suitable package found
            self.unresolved.push(dep.name);
            Ok(false)
        }
    }

    /// Check if there are any packages to install.
    pub fn is_empty(&self) -> bool {
        self.batches.values().all(|v| v.is_empty())
    }

    /// Get the total number of packages to install.
    pub fn package_count(&self) -> usize {
        self.batches.values().map(|v| v.len()).sum()
    }

    /// Get a list of unresolved dependencies.
    pub fn unresolved(&self) -> &[&'static str] {
        &self.unresolved
    }

    /// Build a message listing all packages to be installed.
    pub fn build_install_message(&self) -> String {
        let mut msg = String::from("The following packages will be installed:\n\n");

        // Sort managers by priority
        let mut managers: Vec<_> = self.batches.keys().collect();
        managers.sort_by_key(|m| m.priority());

        for manager in managers {
            let packages = &self.batches[manager];
            if packages.is_empty() {
                continue;
            }

            msg.push_str(&format!("**{}**:\n", manager.display_name()));
            for pkg in packages {
                msg.push_str(&format!(
                    "  • {} ({})\n",
                    pkg.dependency_name, pkg.package_def.package_name
                ));
            }
            msg.push('\n');
        }

        msg
    }

    /// Prompt the user for confirmation to install all packages.
    pub fn prompt_confirmation(&self) -> Result<bool> {
        let count = self.package_count();
        if count == 0 {
            return Ok(true);
        }

        let mut msg = self.build_install_message();
        let (question, yes_text) = if count == 1 {
            ("\nDo you want to install it?", "Install")
        } else {
            ("\nDo you want to install all of them?", "Install All")
        };
        msg.push_str(question);

        let should_install = FzfWrapper::builder()
            .confirm(&msg)
            .yes_text(yes_text)
            .no_text("Cancel")
            .show_confirmation()?;

        Ok(matches!(
            should_install,
            crate::menu_utils::ConfirmResult::Yes
        ))
    }

    /// Execute the batched installation.
    ///
    /// Installs packages in priority order (native managers first, then Flatpak, etc.)
    pub fn execute(&self) -> Result<PackageStatus> {
        if self.is_empty() {
            return Ok(PackageStatus::Installed);
        }

        // Sort managers by priority
        let mut managers: Vec<_> = self.batches.keys().collect();
        managers.sort_by_key(|m| m.priority());

        for manager in managers {
            let packages = &self.batches[manager];
            if packages.is_empty() {
                continue;
            }

            // Show installation progress message
            let installing_msg = format!(
                "Installing {} package{} via {}...",
                packages.len(),
                if packages.len() == 1 { "" } else { "s" },
                manager.display_name()
            );

            FzfWrapper::builder()
                .message(&installing_msg)
                .title("Installing Packages")
                .show_message()?;

            // Execute installation for this manager
            super::install::install_packages(*manager, packages)?;
        }

        Ok(PackageStatus::Installed)
    }
}

/// Ensure multiple dependencies are installed with batched prompts.
///
/// This is the main entry point for installing multiple dependencies at once.
/// It groups packages by manager and prompts once per batch.
///
/// # Returns
///
/// - `Ok(PackageStatus::Installed)` if all packages are installed or were successfully installed
/// - `Ok(PackageStatus::Declined)` if user cancelled
/// - `Ok(PackageStatus::Failed)` if any installation failed
pub fn ensure_dependencies_batch(deps: &'static [Dependency]) -> Result<PackageStatus> {
    let mut batch = InstallBatch::new();

    for dep in deps {
        batch.add(dep)?;
    }

    // Check for unresolved dependencies
    if !batch.unresolved().is_empty() {
        let msg = format!(
            "The following dependencies cannot be automatically installed:\n\n{}",
            batch
                .unresolved()
                .iter()
                .map(|s| format!("  • {}", s))
                .collect::<Vec<_>>()
                .join("\n")
        );
        FzfWrapper::builder()
            .message(&msg)
            .title("Manual Installation Required")
            .show_message()?;
    }

    if batch.is_empty() {
        return Ok(PackageStatus::Installed);
    }

    // Prompt for confirmation
    if !batch.prompt_confirmation()? {
        return Ok(PackageStatus::Declined);
    }

    // Execute installation
    batch.execute()
}
