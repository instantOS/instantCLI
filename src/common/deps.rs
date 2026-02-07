//! Common package dependencies used across the CLI
//!
//! This module provides Dependency definitions for packages used across
//! multiple modules (arch installer, game saves, doctor checks, etc.)

use crate::common::package::{Dependency, PackageDefinition, PackageManager};
use crate::common::requirements::InstallTest;

// =============================================================================
// Core CLI Dependencies
// =============================================================================

pub static FZF: Dependency = Dependency {
    name: "fzf",
    packages: &[
        PackageDefinition::new("fzf", PackageManager::Pacman),
        PackageDefinition::new("fzf", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("fzf")],
};

pub static GIT: Dependency = Dependency {
    name: "git",
    packages: &[
        PackageDefinition::new("git", PackageManager::Pacman),
        PackageDefinition::new("git", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("git")],
};

pub static LIBGIT2: Dependency = Dependency {
    name: "libgit2",
    packages: &[
        PackageDefinition::new("libgit2", PackageManager::Pacman),
        PackageDefinition::new("libgit2-dev", PackageManager::Apt),
    ],
    tests: &[InstallTest::FileExists("/usr/lib/libgit2.so")],
};

pub static GUM: Dependency = Dependency {
    name: "gum",
    packages: &[PackageDefinition::new("gum", PackageManager::Pacman)],
    tests: &[InstallTest::WhichSucceeds("gum")],
};

pub static CFDISK: Dependency = Dependency {
    name: "cfdisk",
    packages: &[
        PackageDefinition::new("util-linux", PackageManager::Pacman),
        PackageDefinition::new("fdisk", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("cfdisk")],
};

// =============================================================================
// System Upgrade
// =============================================================================

pub static TOPGRADE: Dependency = Dependency {
    name: "topgrade",
    packages: &[PackageDefinition::new("topgrade", PackageManager::Pacman)],
    tests: &[InstallTest::WhichSucceeds("topgrade")],
};

// =============================================================================
// Game Save Management
// =============================================================================

pub static RESTIC: Dependency = Dependency {
    name: "restic",
    packages: &[
        PackageDefinition::new("restic", PackageManager::Pacman),
        PackageDefinition::new("restic", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("restic")],
};

// =============================================================================
// Doctor Checks
// =============================================================================

pub static PACMAN_CONTRIB: Dependency = Dependency {
    name: "pacman-contrib",
    packages: &[PackageDefinition::new(
        "pacman-contrib",
        PackageManager::Pacman,
    )],
    tests: &[
        InstallTest::WhichSucceeds("paccache"),
        InstallTest::WhichSucceeds("checkupdates"),
    ],
};

pub static SMARTMONTOOLS: Dependency = Dependency {
    name: "smartmontools",
    packages: &[
        PackageDefinition::new("smartmontools", PackageManager::Pacman),
        PackageDefinition::new("smartmontools", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("smartctl")],
};
