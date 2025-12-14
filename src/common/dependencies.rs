//! DEPRECATED: Use `crate::common::package::Dependency` instead.
//!
//! This module is kept for backward compatibility but should not be used
//! in new code. The new unified package system in `common::package` supports
//! multiple package managers (Pacman, Apt, Dnf, Zypper, Flatpak, AUR, Cargo).

use crate::common::requirements::{FlatpakPackage, InstallTest, RequiredPackage};

#[deprecated(note = "Use `crate::common::package::Dependency` instead")]
#[derive(Debug, Clone)]
pub enum Package {
    Os(&'static RequiredPackage),
    Flatpak(&'static FlatpakPackage),
}

#[deprecated(note = "Use `crate::common::package::Dependency` instead")]
#[derive(Debug, Clone)]
pub struct Dependency {
    pub package: Package,
    pub checks: &'static [InstallTest],
}

impl Dependency {
    pub const fn os(package: &'static RequiredPackage) -> Self {
        Self {
            package: Package::Os(package),
            checks: package.tests,
        }
    }

    pub const fn flatpak(flatpak: &'static FlatpakPackage) -> Self {
        Self {
            package: Package::Flatpak(flatpak),
            checks: flatpak.tests,
        }
    }

    #[allow(dead_code)]
    pub const fn custom(package: Package, checks: &'static [InstallTest]) -> Self {
        Self { package, checks }
    }

    pub fn is_satisfied(&self) -> bool {
        self.checks.iter().all(|check| (*check).run())
    }
}
