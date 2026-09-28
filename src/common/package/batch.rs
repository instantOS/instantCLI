//! Batched package installation.
//!
//! This module provides functionality to batch multiple package installations
//! by package manager, reducing the number of prompts and sudo invocations.

use std::collections::HashMap;

use anyhow::Result;

use super::{Dependency, PackageDefinition, PackageManager};
use crate::menu_utils::{FzfWrapper, Header, HeaderBuilder};
use crate::ui::catppuccin::{colors, hex_to_ansi_fg};
use crate::ui::nerd_font::NerdFont;

/// A batch of packages to install, grouped by package manager.
#[derive(Debug, Default)]
pub struct InstallBatch {
    /// Packages grouped by manager
    batches: HashMap<PackageManager, Vec<PackageToInstall>>,
}

/// Information about a package to install
#[derive(Debug)]
pub(crate) struct PackageToInstall {
    /// Human-readable name of the dependency
    pub dependency_name: &'static str,
    /// The package definition to install
    pub package_def: &'static PackageDefinition,
}

/// Whether a batch may install without asking first.
///
/// [`Prompt`](Self::Prompt) is the default everywhere a person is present and
/// the machine is theirs. [`Auto`](Self::Auto) is for installing onto a host
/// that is throwaway — the live ISO, whose `/etc` is RAM — where asking
/// permission to modify it would be theatre and there may be nobody at the
/// keyboard to answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirmation {
    /// Ask, via a dialog.
    Prompt,
    /// Proceed without asking, having said what is about to happen.
    Auto,
}

impl InstallBatch {
    /// Create a new empty install batch.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a dependency whose installation state and package choice were
    /// already resolved by the caller.
    pub(crate) fn add_resolved(
        &mut self,
        dep: &'static Dependency,
        package_def: &'static PackageDefinition,
    ) {
        self.batches
            .entry(package_def.manager)
            .or_default()
            .push(PackageToInstall {
                dependency_name: dep.name,
                package_def,
            });
    }

    /// Check if there are any packages to install.
    pub fn is_empty(&self) -> bool {
        self.batches.values().all(|v| v.is_empty())
    }

    /// Get the total number of packages to install.
    pub fn package_count(&self) -> usize {
        self.batches.values().map(|v| v.len()).sum()
    }

    /// Build a styled FZF menu header listing all packages to be installed.
    pub fn build_install_header(&self) -> Header {
        let count = self.package_count();
        let title = if count == 1 {
            "Package Installation"
        } else {
            "Package Installations"
        };

        let mut builder = HeaderBuilder::new(NerdFont::Package, title)
            .subtitle("The following packages will be installed:");

        // Sort managers by priority
        let mut managers: Vec<_> = self.batches.keys().collect();
        managers.sort_by_key(|m| m.priority());

        let subtext_color = hex_to_ansi_fg(colors::SUBTEXT0);
        let reset = "\x1b[0m";

        for manager in managers {
            let packages = &self.batches[manager];
            if packages.is_empty() {
                continue;
            }

            builder = builder.section(manager.display_name());
            for pkg in packages {
                let pkg_line = if pkg.dependency_name == pkg.package_def.package_name {
                    format!("  • {}", pkg.dependency_name)
                } else {
                    format!(
                        "  • {} {subtext_color}({}){reset}",
                        pkg.dependency_name, pkg.package_def.package_name
                    )
                };
                builder = builder.line(pkg_line);
            }
        }

        builder.build()
    }

    /// The packages this batch would install, as `dependency → package` names.
    pub fn planned(&self) -> Vec<(&'static str, &'static str)> {
        self.batches
            .values()
            .flatten()
            .map(|entry| (entry.dependency_name, entry.package_def.package_name))
            .collect()
    }

    /// Whether to go ahead, honouring `confirmation`.
    ///
    /// [`Confirmation::Auto`] still reports what it is about to do — silently
    /// installing is worse than unasked installing — it just does not wait.
    pub fn confirm(&self, confirmation: Confirmation) -> Result<bool> {
        if self.is_empty() {
            return Ok(true);
        }
        if confirmation == Confirmation::Auto {
            for (dependency, package) in self.planned() {
                println!("Installing missing dependency: {dependency} ({package})");
            }
            return Ok(true);
        }
        self.prompt_confirmation()
    }

    /// Prompt the user for confirmation to install all packages.
    pub fn prompt_confirmation(&self) -> Result<bool> {
        let count = self.package_count();
        if count == 0 {
            return Ok(true);
        }

        let header = self.build_install_header();
        let (question, yes_text) = if count == 1 {
            ("Do you want to install it?", "Install")
        } else {
            ("Do you want to install all of them?", "Install All")
        };

        let should_install = FzfWrapper::builder()
            .header(header)
            .confirm(question)
            .yes_text(yes_text)
            .no_text("Cancel")
            .confirm_dialog()?;

        Ok(matches!(
            should_install,
            crate::menu_utils::ConfirmResult::Yes
        ))
    }

    /// Execute the batched installation.
    ///
    /// Installs packages in priority order (native managers first, then Flatpak, etc.)
    pub fn execute(&self) -> Result<()> {
        if self.is_empty() {
            return Ok(());
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
                .message_dialog()?;

            // Execute installation for this manager
            super::install::install_packages(*manager, packages)?;
        }

        Ok(())
    }
}
