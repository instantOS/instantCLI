//! Alternative source selection for dotfiles.
//!
//! Allows users to select which repository/subdirectory a dotfile is sourced from.

mod action;
mod apply;
mod browse;
mod create_flow;
mod direct;
mod discovery;
mod flow;
mod handle;
mod lists;
mod picker;
mod select_flow;

pub use apply::add_to_destination;
pub use create_flow::pick_destination_and_add;
pub use handle::handle_alternative;

use crate::dot::override_config::DotfileSource;

pub(crate) fn default_source_for(sources: &[DotfileSource]) -> Option<DotfileSource> {
    sources.first().cloned()
}
