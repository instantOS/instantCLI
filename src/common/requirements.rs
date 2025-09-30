use std::path::Path;

use anyhow::Result;
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

impl RequiredPackage {
    pub fn is_installed(&self) -> bool {
        self.tests.iter().any(|test| test.run())
    }

    pub fn ensure(&self) -> Result<bool> {
        Ok(self.is_installed())
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
