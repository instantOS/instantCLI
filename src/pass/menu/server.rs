use anyhow::{Result, anyhow, bail};

use crate::menu::client::HostedMenuClient;
use crate::menu::protocol::ChoiceOptions;

use super::super::browser::build_quick_access_menu_items;
use super::super::dependencies::ensure_core_dependencies;
use super::super::sort::{PassSort, sort_entries};
use super::super::store::{ensure_password_store_dir, load_entries};
use super::copy_entry;

pub(crate) fn run() -> Result<i32> {
    ensure_core_dependencies()?;

    let client = HostedMenuClient::new();

    let store_dir = ensure_password_store_dir()?;

    loop {
        let mut entries = load_entries(&store_dir)?;
        sort_entries(&mut entries, PassSort::quick_access_default())?;
        let menu_items = build_quick_access_menu_items(&entries);

        let selected = match client.choice(ChoiceOptions::new("Pass"), menu_items)? {
            crate::menu_utils::DialogOutcome::Submitted(selected) => selected,
            crate::menu_utils::DialogOutcome::Cancelled => return Ok(1),
        };

        let metadata = selected
            .items
            .first()
            .and_then(|item| item.metadata.as_ref())
            .ok_or_else(|| anyhow!("Selected menu item missing metadata"))?;

        match metadata.get("kind").map(String::as_str) {
            Some("entry") => {
                let key = metadata
                    .get("key")
                    .ok_or_else(|| anyhow!("Entry item missing key metadata"))?;
                copy_entry(&entries, key)?;
                // Exit after copying so the user can paste immediately.
                return Ok(0);
            }
            Some("menu") => {
                crate::common::terminal::launch_menu_in_terminal("pass", "Pass", &[], false)?;
                return Ok(0);
            }
            Some("close") => return Ok(0),
            Some("folder" | "add" | "back") => {}
            other => bail!("Unknown menu item kind: {:?}", other),
        }
    }
}
