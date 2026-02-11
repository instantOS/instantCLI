//! Game discovery modules for different emulators
//!
//! Each submodule scans filesystem locations used by a specific emulator
//! to discover games/saves that can be tracked.

pub mod azahar;
pub mod duckstation;
pub mod eden;
pub mod pcsx2;

use std::path::PathBuf;

use anyhow::Result;

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

    /// Mark this game as already tracked under the given name
    fn set_existing(&mut self, tracked_name: String);

    fn build_preview(&self) -> FzfPreview;
    fn build_launch_command(&self) -> Option<String>;
    fn clone_box(&self) -> Box<dyn DiscoveredGame>;
}

impl Clone for Box<dyn DiscoveredGame> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// Discover games from a single platform, boxing the results.
fn collect_from<T: DiscoveredGame + 'static>(
    is_installed: fn() -> bool,
    discover: fn() -> Result<Vec<T>>,
    results: &mut Vec<Box<dyn DiscoveredGame>>,
) -> Result<()> {
    if is_installed() {
        results.extend(discover()?.into_iter().map(|g| Box::new(g) as _));
    }
    Ok(())
}

/// Discover all games from all installed platforms.
///
/// Returns an empty Vec if no supported platforms are installed.
pub fn discover_all() -> Result<Vec<Box<dyn DiscoveredGame>>> {
    let mut results = Vec::new();
    collect_from(
        eden::is_eden_installed,
        eden::discover_eden_games,
        &mut results,
    )?;
    collect_from(
        pcsx2::is_pcsx2_installed,
        pcsx2::discover_pcsx2_memcards,
        &mut results,
    )?;
    collect_from(
        duckstation::is_duckstation_installed,
        duckstation::discover_duckstation_memcards,
        &mut results,
    )?;
    collect_from(
        azahar::is_azahar_installed,
        azahar::discover_azahar_games,
        &mut results,
    )?;
    Ok(results)
}
