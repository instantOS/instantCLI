//! Ludusavi manifest integration for wine prefix save discovery
//!
//! Downloads and caches the Ludusavi manifest, then scans wine prefixes
//! to discover save games by matching manifest paths against actual
//! filesystem contents — no game name matching required.

pub mod manifest;
mod scanner;
pub mod types;

pub use scanner::{scan_wine_prefix, stream_wine_prefix_games};
pub use types::{DiscoveredWineSave, choose_primary_save};
