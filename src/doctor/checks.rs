//! Health check implementations for the doctor module
//! 
//! This module has been refactored into multiple files by functionality:
//! - network.rs: Network-related checks (internet, repository configuration)
//! - locale.rs: Locale and language configuration checks
//! - system.rs: System-level checks (swap, updates)
//! - storage.rs: Storage-related checks (pacman cache, disk health)
//! - display.rs: Display and compositor checks
//! - security.rs: Security-related checks (polkit agents)

use crate::doctor::{CheckStatus, DoctorCheck, PrivilegeLevel};

pub mod network;
pub mod locale;
pub mod system;
pub mod storage;
pub mod display;
pub mod security;

// Re-export all check types for easy access
pub use network::{InternetCheck, InstantRepoCheck};
pub use locale::LocaleCheck;
pub use system::{SwapCheck, PendingUpdatesCheck};
pub use storage::{
    PacmanCacheCheck, PacmanStaleDownloadsCheck, SmartHealthCheck, 
    PacmanDbSyncCheck, YayCacheCheck
};
pub use display::SwayDisplayCheck;
pub use security::PolkitAgentCheck;