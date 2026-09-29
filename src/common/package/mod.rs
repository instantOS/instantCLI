//! Unified package management system for multi-distro support.
//!
//! A [`Dependency`] can be satisfied by packages from several [`PackageManager`]s.
//!
//! # Priority
//!
//! Package managers are tried in this order:
//! 1. Native package managers - highest priority
//! 2. Flatpak and Snap - prebuilt, sandboxed
//! 3. AUR - compiles from source
//! 4. Cargo - compiles from source, most resource intensive
//!
//! See [`Dependency`] for an example and `dep!` for concise definitions.

mod batch;
mod definition;
mod dependency;
mod install;
mod macros;
mod manager;
mod removal;

pub use definition::PackageDefinition;
pub use dependency::{Dependency, InstallResult, ensure_all, ensure_all_auto};
pub use install::{install_package_names, uninstall_packages};
pub use manager::{PackageManager, detect_aur_helper};
pub use removal::removal_cascade;
