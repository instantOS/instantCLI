use anyhow::Result;

use crate::menu_utils::{FzfWrapper, HeaderBuilder, MenuCursor};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::super::dependencies::ensure_otp_dependency;
use super::super::operations::{
    copy_otp_entry, copy_password_entry, delete_entry, edit_password_entry, export_entry,
    generate_password_entry, rename_entry_interactive, upsert_otp_entry_interactive,
};
use super::super::store::{ensure_password_store_dir, load_entries, record_frecency};
use super::super::types::{DEFAULT_PASSWORD_LENGTH, EditAction, EditActionItem, PassEntry};
use super::move_entry;

pub(super) fn run(entry: &PassEntry) -> Result<Option<String>> {
    let mut cursor = MenuCursor::new();
    let store_dir = ensure_password_store_dir()?;

    loop {
        let refreshed_entries = load_entries(&store_dir)?;
        let current_entry = match refreshed_entries
            .iter()
            .find(|current| current.display_name == entry.display_name)
        {
            Some(entry) => entry.clone(),
            None => return Ok(None),
        };

        let items = build_items(&current_entry);
        let mut builder = FzfWrapper::builder()
            .header(
                HeaderBuilder::new(NerdFont::Edit, "Edit Password Entry")
                    .field("Entry", &current_entry.display_name)
                    .build(),
            )
            .prompt("Action")
            .responsive_layout();

        if let Some(index) = cursor.initial_index(&items) {
            builder = builder.initial_index(index);
        }

        match builder.items(items.clone()).padded().select_one()? {
            crate::menu_utils::DialogOutcome::Submitted(item) => {
                cursor.update(&item, &items);
                match item.action {
                    EditAction::CopyPassword => {
                        copy_password_entry(&current_entry)?;
                        record_frecency(&current_entry.display_name)?;
                    }
                    EditAction::CopyOtp => {
                        copy_otp_entry(&current_entry)?;
                        record_frecency(&current_entry.display_name)?;
                    }
                    EditAction::Export => {
                        export_entry(&current_entry, None)?;
                    }
                    EditAction::Rename => {
                        rename_entry_interactive(&current_entry)?;
                    }
                    EditAction::Move => {
                        if let Some(key) = move_entry::run(&store_dir, &current_entry)? {
                            return Ok(Some(key));
                        }
                    }
                    EditAction::EditPassword => {
                        if let Err(error) = edit_password_entry(&current_entry) {
                            FzfWrapper::message(&format!("{error:#}"))?;
                        }
                    }
                    EditAction::GeneratePassword => {
                        generate_password_entry(
                            Some(current_entry.primary_key().to_string()),
                            DEFAULT_PASSWORD_LENGTH,
                        )?;
                    }
                    EditAction::EditOtp => {
                        ensure_otp_dependency()?;
                        upsert_otp_entry_interactive(&current_entry)?;
                    }
                    EditAction::Delete => {
                        delete_entry(&current_entry)?;
                        return Ok(None);
                    }
                    EditAction::Back => return Ok(None),
                }
            }
            crate::menu_utils::DialogOutcome::Cancelled => return Ok(None),
        }
    }
}

fn build_items(entry: &PassEntry) -> Vec<EditActionItem> {
    let mut items = Vec::new();

    if entry.has_secret() {
        items.push(EditActionItem {
            key: "copy-password",
            display: format!(
                "{} Copy Password",
                format_icon_colored(NerdFont::Clipboard, colors::GREEN)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Clipboard, "Copy Password")
                .text("Copy the first line of the pass entry to the clipboard.")
                .build(),
            action: EditAction::CopyPassword,
        });
    }

    if entry.has_otp() {
        items.push(EditActionItem {
            key: "copy-otp",
            display: format!(
                "{} Copy OTP",
                format_icon_colored(NerdFont::Clock, colors::TEAL)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Clock, "Copy OTP")
                .text("Generate the current OTP code and copy it to the clipboard.")
                .build(),
            action: EditAction::CopyOtp,
        });
    }

    items.push(EditActionItem {
        key: "export",
        display: format!(
            "{} Export",
            format_icon_colored(NerdFont::Upload, colors::LAVENDER)
        ),
        preview: PreviewBuilder::new()
            .header(NerdFont::Upload, "Export Entry")
            .text("Write the decrypted entry content to a file.")
            .build(),
        action: EditAction::Export,
    });

    items.push(EditActionItem {
        key: "rename",
        display: format!(
            "{} Rename",
            format_icon_colored(NerdFont::Edit, colors::BLUE)
        ),
        preview: PreviewBuilder::new()
            .header(NerdFont::Edit, "Rename Entry")
            .text("Rename the selected pass entry.")
            .build(),
        action: EditAction::Rename,
    });

    items.push(EditActionItem {
        key: "move",
        display: format!(
            "{} Move to Folder",
            format_icon_colored(NerdFont::Folder, colors::BLUE)
        ),
        preview: PreviewBuilder::new()
            .header(NerdFont::Folder, "Move to Folder")
            .text("Move this entry to an existing or new folder in the password store.")
            .build(),
        action: EditAction::Move,
    });

    if entry.has_secret() {
        items.push(EditActionItem {
            key: "edit-password",
            display: format!(
                "{} Edit Password",
                format_icon_colored(NerdFont::Key, colors::PEACH)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Key, "Edit Password")
                .text("Open the entry in Neovim with swap files disabled.")
                .build(),
            action: EditAction::EditPassword,
        });

        items.push(EditActionItem {
            key: "generate-password",
            display: format!(
                "{} Generate Password",
                format_icon_colored(NerdFont::Refresh, colors::SAPPHIRE)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Refresh, "Generate Password")
                .text("Generate a fresh password into this entry.")
                .build(),
            action: EditAction::GeneratePassword,
        });
    }

    if entry.has_otp() {
        items.push(EditActionItem {
            key: "edit-otp",
            display: format!(
                "{} Edit OTP",
                format_icon_colored(NerdFont::Clock, colors::TEAL)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Clock, "Edit OTP")
                .text("Replace the stored OTP URI for this entry.")
                .build(),
            action: EditAction::EditOtp,
        });
    }

    items.push(EditActionItem {
        key: "delete",
        display: format!(
            "{} Delete",
            format_icon_colored(NerdFont::Trash, colors::RED)
        ),
        preview: PreviewBuilder::new()
            .header(NerdFont::Trash, "Delete Entry")
            .text("Remove the selected pass entry with confirmation.")
            .build(),
        action: EditAction::Delete,
    });

    items.push(EditActionItem {
        key: "back",
        display: format!("{} Back", format_back_icon()),
        preview: PreviewBuilder::new()
            .header(NerdFont::ArrowLeft, "Back")
            .text("Return to the tree view.")
            .build(),
        action: EditAction::Back,
    });

    items
}
