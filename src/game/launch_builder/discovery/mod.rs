//! Game discovery modules for different emulators
//!
//! Each submodule scans filesystem locations used by a specific emulator
//! to discover games/saves that can be tracked.

pub mod azahar;
pub mod duckstation;
pub mod eden;
pub mod pcsx2;

use std::path::PathBuf;

use crate::menu::protocol::FzfPreview;

/// Common trait for discovered games across different emulators
pub trait DiscoveredGame: std::fmt::Debug {
    fn display_name(&self) -> &str;
    fn save_path(&self) -> &PathBuf;
    fn game_path(&self) -> Option<&PathBuf>;
    fn platform_name(&self) -> &'static str;
    fn platform_short(&self) -> &'static str;
    fn unique_key(&self) -> String;
    fn is_existing(&self) -> bool;
    fn tracked_name(&self) -> Option<&str>;

    fn build_preview(&self) -> FzfPreview;
    fn build_launch_command(&self) -> Option<String>;
    fn clone_box(&self) -> Box<dyn DiscoveredGame>;
}

impl Clone for Box<dyn DiscoveredGame> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}
