mod types;

use anyhow::Result;

use crate::menu_utils::{FzfResult, FzfWrapper, Header, MenuCursor};
use crate::ui::catppuccin::fzf_mocha_args;

use types::DevMenuEntry;

pub async fn dev_menu(debug: bool) -> Result<()> {
    let mut cursor = MenuCursor::new();
    loop {
        let entry = match select_dev_menu_entry(&mut cursor)? {
            Some(entry) => entry,
            None => return Ok(()),
        };

        match entry {
            DevMenuEntry::Clone => super::handle_clone_internal(debug).await?,
            DevMenuEntry::Install => super::handle_install(debug).await?,
            DevMenuEntry::Setup => super::setup::handle_setup(debug).await?,
            DevMenuEntry::CloseMenu => return Ok(()),
        }
    }
}

fn select_dev_menu_entry(cursor: &mut MenuCursor) -> Result<Option<DevMenuEntry>> {
    let entries = vec![
        DevMenuEntry::Clone,
        DevMenuEntry::Install,
        DevMenuEntry::Setup,
        DevMenuEntry::CloseMenu,
    ];

    let mut builder = FzfWrapper::builder()
        .header(Header::fancy("Dev Menu"))
        .prompt("Select")
        .args(fzf_mocha_args())
        .responsive_layout();

    if let Some(index) = cursor.initial_index(&entries) {
        builder = builder.initial_index(index);
    }

    let result = builder.select(entries.clone())?;

    match result {
        FzfResult::Selected(entry) => {
            cursor.update(&entry, &entries);
            Ok(Some(entry))
        }
        FzfResult::Cancelled => Ok(None),
        _ => Ok(None),
    }
}
