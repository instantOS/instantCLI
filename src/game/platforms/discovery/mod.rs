//! Game discovery modules for different emulators
//!
//! Each submodule scans filesystem locations used by a specific emulator
//! to discover games/saves that can be tracked.

pub mod azahar;
pub mod duckstation;
pub mod eden;
pub mod epic;
pub mod faugus;
pub mod pcsx2;
pub mod steam;
pub mod wine;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoverySource {
    Switch,
    Ps2,
    Ps1,
    ThreeDs,
    Epic,
    Steam,
    Faugus,
    Wine,
}

pub const DEFAULT_DISCOVERY_SOURCES: [DiscoverySource; 8] = [
    DiscoverySource::Switch,
    DiscoverySource::Ps2,
    DiscoverySource::Ps1,
    DiscoverySource::ThreeDs,
    DiscoverySource::Epic,
    DiscoverySource::Steam,
    DiscoverySource::Faugus,
    DiscoverySource::Wine,
];

pub fn active_sources(sources: &[DiscoverySource]) -> &[DiscoverySource] {
    if sources.is_empty() {
        &DEFAULT_DISCOVERY_SOURCES
    } else {
        sources
    }
}

fn emit_discovered_games<T, F>(
    is_installed: fn() -> bool,
    discover: fn() -> Result<Vec<T>>,
    mut on_game: F,
) -> Result<()>
where
    T: DiscoveredGame + 'static,
    F: FnMut(Box<dyn DiscoveredGame>) -> Result<()>,
{
    if !is_installed() {
        return Ok(());
    }

    for game in discover()? {
        on_game(Box::new(game))?;
    }

    Ok(())
}

fn source_label(source: DiscoverySource) -> &'static str {
    match source {
        DiscoverySource::Switch => "Scanning Nintendo Switch saves",
        DiscoverySource::Ps2 => "Scanning PS2 saves",
        DiscoverySource::Ps1 => "Scanning PS1 saves",
        DiscoverySource::ThreeDs => "Scanning 3DS saves",
        DiscoverySource::Epic => "Scanning Epic Games prefixes",
        DiscoverySource::Steam => "Scanning Steam Proton prefixes",
        DiscoverySource::Faugus => "Scanning Faugus Launcher prefixes",
        DiscoverySource::Wine => "Scanning generic Wine prefixes",
    }
}

pub enum DiscoveryEvent {
    SourceStarted {
        index: usize,
        total: usize,
        label: &'static str,
    },
    GameFound(Box<dyn DiscoveredGame>),
}

pub fn discover_selected_events<F>(sources: &[DiscoverySource], mut on_event: F) -> Result<()>
where
    F: FnMut(DiscoveryEvent) -> Result<()>,
{
    let active_sources = active_sources(sources);
    let total = active_sources.len();

    for (index, source) in active_sources.iter().copied().enumerate() {
        on_event(DiscoveryEvent::SourceStarted {
            index,
            total,
            label: source_label(source),
        })?;

        match source {
            DiscoverySource::Switch => emit_discovered_games(
                eden::is_eden_installed,
                eden::discover_eden_games,
                &mut |game| on_event(DiscoveryEvent::GameFound(game)),
            )?,
            DiscoverySource::Ps2 => emit_discovered_games(
                pcsx2::is_pcsx2_installed,
                pcsx2::discover_pcsx2_memcards,
                &mut |game| on_event(DiscoveryEvent::GameFound(game)),
            )?,
            DiscoverySource::Ps1 => emit_discovered_games(
                duckstation::is_duckstation_installed,
                duckstation::discover_duckstation_memcards,
                &mut |game| on_event(DiscoveryEvent::GameFound(game)),
            )?,
            DiscoverySource::ThreeDs => emit_discovered_games(
                azahar::is_azahar_installed,
                azahar::discover_azahar_games,
                &mut |game| on_event(DiscoveryEvent::GameFound(game)),
            )?,
            DiscoverySource::Epic => {
                epic::stream_discover_epic_games(|game| {
                    on_event(DiscoveryEvent::GameFound(Box::new(game)))
                })?;
            }
            DiscoverySource::Steam => {
                if steam::is_steam_installed() {
                    steam::stream_discover_steam_games(|game| {
                        on_event(DiscoveryEvent::GameFound(Box::new(game)))
                    })?;
                }
            }
            DiscoverySource::Faugus => emit_discovered_games(
                faugus::is_faugus_installed,
                faugus::discover_faugus_games,
                &mut |game| on_event(DiscoveryEvent::GameFound(game)),
            )?,
            DiscoverySource::Wine => {
                if wine::is_wine_installed() {
                    wine::stream_discover_wine_games(|game| {
                        on_event(DiscoveryEvent::GameFound(Box::new(game)))
                    })?;
                }
            }
        }
    }

    Ok(())
}
