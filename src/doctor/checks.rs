//! Health check implementations for the doctor module
//!
//! This module has been refactored into multiple files by functionality:
//! - network.rs: Network-related checks (internet, repository configuration)
//! - locale.rs: Locale and language configuration checks
//! - system.rs: System-level checks (swap, updates)
//! - storage.rs: Storage-related checks (pacman cache, disk health)
//! - display.rs: Display and compositor checks
//! - security.rs: Security-related checks (polkit agents)
//! - nerdfont.rs: Nerd Font symbol rendering checks
//! - completions.rs: Shell completion checks

use crate::doctor::{CheckStatus, DoctorCheck, PrivilegeLevel};

pub mod completions;
pub mod display;
pub mod energy;
pub mod locale;
pub mod nerdfont;
pub mod network;
mod performance;
pub mod security;
pub mod storage;
pub mod system;
pub mod tools;

// Re-export all check types for easy access
pub use completions::ShellCompletionCheck;
pub use display::SwayDisplayCheck;
pub use energy::PowerCheck;
pub use locale::LocaleCheck;
pub use nerdfont::NerdFontCheck;
pub use network::{InstantRepoCheck, InternetCheck};
pub use performance::PerformanceTest;
pub use security::PolkitAgentCheck;
pub use storage::{
    PacmanCacheCheck, PacmanDbSyncCheck, PacmanStaleDownloadsCheck, SmartHealthCheck, YayCacheCheck,
};
pub use system::{PendingUpdatesCheck, SwapCheck};
pub use tools::{BatCheck, GitConfigCheck};
