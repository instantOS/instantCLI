//! Unified package management system for multi-distro support.
//!
//! This module provides a unified abstraction for package management across
//! multiple Linux distributions and cross-platform package managers.
//!
//! # Architecture
//!
//! - [`PackageManager`]: Enum representing all supported package managers
//! - [`PackageDefinition`]: How to install a package with a specific manager
//! - [`Dependency`]: A dependency that can be satisfied by multiple packages
//!
//! # Priority
//!
//! Package managers are tried in this order:
//! 1. Native package managers (Pacman, Apt, Dnf, Zypper) - highest priority
//! 2. Flatpak - prebuilt, sandboxed
//! 3. AUR - compiles from source
//! 4. Cargo - compiles from source, most resource intensive
//!
//! # Example
//!
//! ```ignore
//! use crate::common::package::{Dependency, PackageDefinition, PackageManager};
//! use crate::common::requirements::InstallTest;
//!
//! static FIREFOX: Dependency = Dependency {
//!     name: "Firefox",
//!     description: Some("Mozilla Firefox web browser"),
//!     packages: &[
//!         PackageDefinition::new("firefox", PackageManager::Pacman),
//!         PackageDefinition::new("firefox", PackageManager::Apt),
//!         PackageDefinition::new("firefox", PackageManager::Dnf),
//!         PackageDefinition::new("org.mozilla.firefox", PackageManager::Flatpak),
//!     ],
//!     tests: &[InstallTest::WhichSucceeds("firefox")],
//! };
//! ```
//!
//! # Migration from Old Types
//!
//! Use the `dep!` macro or `to_dependency()` method on old types:
//!
//! ```ignore
//! // Using the dep! macro for new definitions
//! dep!(FIREFOX, "Firefox", "firefox", flatpak: "org.mozilla.firefox");
//!
//! // Converting existing RequiredPackage
//! let dep = PLAYERCTL.to_dependency();
//! ```

mod batch;
mod definition;
mod dependency;
mod install;
mod legacy;
mod manager;

pub use batch::{ensure_dependencies_batch, InstallBatch};
pub use definition::PackageDefinition;
pub use dependency::{Dependency, InstallResult};
pub use install::PackageInstaller;
pub use manager::{detect_aur_helper, PackageManager};
