//! Package requirement test types.
//!
//! This module contains types for verifying package installation.

use std::path::Path;
use duct::cmd;

/// Status of a package installation request
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageStatus {
    /// Package is installed and ready to use
    Installed,
    /// User explicitly declined installation
    Declined,
    /// Installation failed or verification failed
    Failed,
}

impl PackageStatus {
    /// Check if the package is effectively installed
    pub fn is_installed(&self) -> bool {
        matches!(self, Self::Installed)
    }
}

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
