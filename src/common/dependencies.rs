use crate::common::requirements::{FlatpakPackage, RequiredPackage};
use anyhow::Result;

#[derive(Debug, Clone)]
pub enum Package {
    Os(&'static RequiredPackage),
    Flatpak(&'static FlatpakPackage),
}

#[derive(Debug, Clone)]
pub struct Dependency {
    pub checks: &'static [fn() -> Result<bool>],
    pub package: Package,
}
