use crate::common::requirements::{FlatpakPackage, InstallTest, RequiredPackage};
#[derive(Debug, Clone)]
pub enum Package {
    Os(&'static RequiredPackage),
    Flatpak(&'static FlatpakPackage),
}

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

    pub const fn custom(package: Package, checks: &'static [InstallTest]) -> Self {
        Self { package, checks }
    }

    pub fn is_satisfied(&self) -> bool {
        self.checks.iter().all(|check| (*check).run())
    }
}
