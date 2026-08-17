mod types;

use anyhow::Result;

use crate::menu_utils::{FzfWrapper, HeaderBuilder, MenuCursor};
use crate::ui::nerd_font::NerdFont;

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
            DevMenuEntry::Chroot => super::chroot::handle_chroot(
                super::chroot::ChrootOptions {
                    disk: None,
                    root: None,
                    mountpoint: std::path::PathBuf::from("/mnt/instantos"),
                    shell: "/bin/bash".to_string(),
                    keep_mounted: false,
                },
                debug,
            )?,
            DevMenuEntry::Install => super::handle_install(debug, None).await?,
            DevMenuEntry::Setup => super::setup::handle_setup(debug).await?,
            DevMenuEntry::CloseMenu => return Ok(()),
        }
    }
}

fn select_dev_menu_entry(cursor: &mut MenuCursor) -> Result<Option<DevMenuEntry>> {
    let entries = vec![
        DevMenuEntry::Clone,
        DevMenuEntry::Chroot,
        DevMenuEntry::Install,
        DevMenuEntry::Setup,
        DevMenuEntry::CloseMenu,
    ];

    let header = HeaderBuilder::new(NerdFont::Wrench, "instantOS Development")
        .subtitle("Development tools, chroot environments, and packaging")
        .build();

    let selection = FzfWrapper::builder()
        .header(header)
        .prompt("Select")
        .responsive_layout()
        .cursor(cursor.initial_index(&entries))
        .select_one(entries.clone())?;

    if let Some(ref entry) = selection {
        cursor.update(entry, &entries);
    }
    Ok(selection)
}
