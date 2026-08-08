//! Package dependencies for the video pipeline.
//!
//! These use the unified `Dependency` system so `ins video setup` can check
//! and offer to install system tools that are invoked at render time but were
//! previously never verified.

use crate::common::package::{Dependency, PackageDefinition, PackageManager};
use crate::common::requirements::InstallTest;

/// yt-dlp — downloads music from URLs referenced in music blocks.
pub static YT_DLP: Dependency = Dependency {
    name: "yt-dlp",
    packages: &[
        PackageDefinition::new("yt-dlp", PackageManager::Pacman),
        PackageDefinition::new("yt-dlp", PackageManager::Apt),
        PackageDefinition::new("yt-dlp", PackageManager::Dnf),
    ],
    tests: &[InstallTest::WhichSucceeds("yt-dlp")],
};

/// chromium — headless screenshot engine for slide generation.
pub static CHROMIUM: Dependency = Dependency {
    name: "chromium",
    packages: &[
        PackageDefinition::new("chromium", PackageManager::Pacman),
        PackageDefinition::new("chromium", PackageManager::Apt),
        PackageDefinition::new("chromium", PackageManager::Dnf),
    ],
    tests: &[InstallTest::WhichSucceeds("chromium")],
};

/// pandoc — renders markdown slides to HTML before chromium captures them.
pub static PANDOC: Dependency = Dependency {
    name: "pandoc",
    packages: &[
        PackageDefinition::new("pandoc", PackageManager::Pacman),
        PackageDefinition::new("pandoc", PackageManager::Apt),
        PackageDefinition::new("pandoc", PackageManager::Dnf),
    ],
    tests: &[InstallTest::WhichSucceeds("pandoc")],
};

/// mpv — real-time preview during rendering.
pub static MPV: Dependency = Dependency {
    name: "mpv",
    packages: &[
        PackageDefinition::new("mpv", PackageManager::Pacman),
        PackageDefinition::new("mpv", PackageManager::Apt),
        PackageDefinition::new("mpv", PackageManager::Dnf),
    ],
    tests: &[InstallTest::WhichSucceeds("mpv")],
};

/// All external tools the video pipeline may invoke, in priority order.
pub static ALL: &[&Dependency] = &[&YT_DLP, &CHROMIUM, &PANDOC, &MPV];
