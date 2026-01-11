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
    description: Some("Fuzzy finder for interactive selection"),
    packages: &[
        PackageDefinition::new("fzf", PackageManager::Pacman),
        PackageDefinition::new("fzf", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("fzf")],
};

pub static GIT: Dependency = Dependency {
    name: "git",
    description: Some("Version control system"),
    packages: &[
        PackageDefinition::new("git", PackageManager::Pacman),
        PackageDefinition::new("git", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("git")],
};

pub static LIBGIT2: Dependency = Dependency {
    name: "libgit2",
    description: Some("Git library for native operations"),
    packages: &[
        PackageDefinition::new("libgit2", PackageManager::Pacman),
        PackageDefinition::new("libgit2-dev", PackageManager::Apt),
    ],
    tests: &[InstallTest::FileExists("/usr/lib/libgit2.so")],
};

pub static GUM: Dependency = Dependency {
    name: "gum",
    description: Some("Glamorous shell scripts"),
    packages: &[PackageDefinition::new("gum", PackageManager::Pacman)],
    tests: &[InstallTest::WhichSucceeds("gum")],
};

pub static CFDISK: Dependency = Dependency {
    name: "cfdisk",
    description: Some("Curses-based disk partition table manipulator"),
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
    description: Some("System upgrade tool"),
    packages: &[PackageDefinition::new("topgrade", PackageManager::Pacman)],
    tests: &[InstallTest::WhichSucceeds("topgrade")],
};

// =============================================================================
// Game Save Management
// =============================================================================

pub static RESTIC: Dependency = Dependency {
    name: "restic",
    description: Some("Fast, secure backup program"),
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
    description: Some("Pacman helper utilities"),
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
    description: Some("S.M.A.R.T. disk monitoring tools"),
    packages: &[
        PackageDefinition::new("smartmontools", PackageManager::Pacman),
        PackageDefinition::new("smartmontools", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("smartctl")],
};

pub static POWERPROFILESDAEMON: Dependency = Dependency {
    name: "power-profiles-daemon",
    description: Some("Makes power profiles handling available over D-Bus"),
    packages: &[
        PackageDefinition::new("power-profiles-daemon", PackageManager::Apt),
        PackageDefinition::new("power-profiles-daemon", PackageManager::Pacman),
    ],
    tests: &[InstallTest::WhichSucceeds("powerprofilesctl")],
};
