mod add;
mod entry_actions;
mod instantmenu;
mod move_entry;
mod quick_access;
mod server;
mod tree_view;

use anyhow::Result;

use super::operations::{resolve_entry_by_name, run_primary_action};
use super::store::record_frecency;
use super::types::PassEntry;

pub(super) use add::run_add_menu;
pub(super) use instantmenu::run as interactive_pass_menu_instantmenu;
pub(super) use quick_access::run as interactive_pass_menu;
pub(super) use server::run as interactive_pass_menu_server;
pub(super) use tree_view::interactive_pass_tree_menu;
pub(crate) use tree_view::run_tree_view;

fn copy_entry(entries: &[PassEntry], key: &str) -> Result<()> {
    let entry = resolve_entry_by_name(entries, key, false)?;
    run_primary_action(&entry)?;
    record_frecency(&entry.display_name)?;
    Ok(())
}

fn show_entry_actions(entries: &[PassEntry], key: &str) -> Result<Option<String>> {
    let entry = resolve_entry_by_name(entries, key, false)?;
    entry_actions::run(&entry)
}
