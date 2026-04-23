use anyhow::{Result, anyhow, bail};

use crate::menu::client::MenuClient;
use crate::menu_utils::{FzfResult, FzfWrapper, Header, MenuCursor};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::browser::*;
use super::operations::*;
use super::types::*;
use super::utils::*;

pub(super) fn interactive_pass_menu() -> Result<i32> {
    interactive_pass_quick_access()
}

pub(super) fn interactive_pass_quick_access() -> Result<i32> {
    ensure_core_dependencies()?;

    let store_dir = ensure_password_store_dir()?;
    let mut cursor = MenuCursor::new();

    loop {
        let mut entries = load_entries(&store_dir)?;
        sort_entries_by_frecency(&mut entries)?;
        let quick_access_items = build_quick_access_items(&entries);

        let mut builder = FzfWrapper::builder()
            .header(Header::fancy("Pass"))
            .prompt("Copy")
            .args(fzf_mocha_args())
            .responsive_layout();

        if let Some(index) = cursor.initial_index(&quick_access_items) {
            builder = builder.initial_index(index);
        }

        match builder.select(quick_access_items.clone())? {
            FzfResult::Selected(item) => {
                cursor.update(&item, &quick_access_items);
                match item.kind {
                    BrowserItemKind::Entry(key) => {
                        let entry = resolve_entry_by_name(&entries, &key, false)?;
                        copy_primary_entry(&entry)?;
                        record_frecency(&entry.display_name)?;
                    }
                    BrowserItemKind::Menu => {
                        interactive_pass_tree_menu()?;
                    }
                    BrowserItemKind::Close => return Ok(0),
                    BrowserItemKind::Folder(_)
                    | BrowserItemKind::Add
                    | BrowserItemKind::Edit
                    | BrowserItemKind::Back => {}
                }
            }
            FzfResult::Cancelled => return Ok(1),
            _ => return Ok(1),
        }
    }
}

pub(super) fn interactive_pass_menu_server() -> Result<i32> {
    interactive_pass_quick_access_server()
}

pub(super) fn interactive_pass_quick_access_server() -> Result<i32> {
    ensure_core_dependencies()?;

    let client = MenuClient::new();
    let server_client = client.clone();
    let server_ready = std::thread::spawn(move || server_client.ensure_server_running());

    server_ready
        .join()
        .map_err(|_| anyhow!("menu server thread panicked"))??;

    let store_dir = ensure_password_store_dir()?;

    loop {
        let mut entries = load_entries(&store_dir)?;
        sort_entries_by_frecency(&mut entries)?;
        let menu_items = build_quick_access_menu_items(&entries);

        let selected = client.choice("Pass".to_string(), menu_items, false)?;
        if selected.is_empty() {
            return Ok(1);
        }

        let metadata = selected[0]
            .metadata
            .as_ref()
            .ok_or_else(|| anyhow!("Selected menu item missing metadata"))?;

        match metadata.get("kind").map(String::as_str) {
            Some("entry") => {
                let key = metadata
                    .get("key")
                    .ok_or_else(|| anyhow!("Entry item missing key metadata"))?;
                let entry = resolve_entry_by_name(&entries, key, false)?;
                copy_primary_entry(&entry)?;
                record_frecency(&entry.display_name)?;
            }
            Some("menu") => {
                interactive_pass_tree_menu_server()?;
            }
            Some("close") => return Ok(0),
            Some("folder" | "add" | "edit" | "back") => {}
            other => bail!("Unknown menu item kind: {:?}", other),
        }
    }
}

pub(super) fn interactive_pass_tree_menu() -> Result<i32> {
    ensure_core_dependencies()?;

    let store_dir = ensure_password_store_dir()?;
    let mut path: Vec<String> = Vec::new();
    let mut cursor = MenuCursor::new();

    loop {
        let entries = load_entries(&store_dir)?;
        let browser_items = build_browser_menu_items(&entries, &path, true)?;

        let title = if path.is_empty() {
            "Pass Menu".to_string()
        } else {
            format!("Pass Menu / {}", path.join("/"))
        };

        let mut builder = FzfWrapper::builder()
            .header(Header::fancy(&title))
            .prompt("Select")
            .args(fzf_mocha_args())
            .responsive_layout();

        if let Some(index) = cursor.initial_index(&browser_items) {
            builder = builder.initial_index(index);
        }

        match builder.select(browser_items.clone())? {
            FzfResult::Selected(item) => {
                cursor.update(&item, &browser_items);
                match item.kind {
                    BrowserItemKind::Folder(folder) => {
                        path = path_segments(&folder);
                    }
                    BrowserItemKind::Entry(key) => {
                        let entry = resolve_entry_by_name(&entries, &key, false)?;
                        copy_primary_entry(&entry)?;
                        record_frecency(&entry.display_name)?;
                    }
                    BrowserItemKind::Add => {
                        run_add_menu(path_prefix(&path).as_deref())?;
                    }
                    BrowserItemKind::Edit => {
                        run_edit_browser(path_prefix(&path).as_deref())?;
                    }
                    BrowserItemKind::Back => {
                        if path.is_empty() {
                            return Ok(1);
                        }
                        path.pop();
                    }
                    BrowserItemKind::Close => return Ok(0),
                    BrowserItemKind::Menu => {}
                }
            }
            FzfResult::Cancelled => {
                if path.is_empty() {
                    return Ok(1);
                }
                path.pop();
            }
            _ => return Ok(1),
        }
    }
}

pub(super) fn interactive_pass_tree_menu_server() -> Result<i32> {
    ensure_core_dependencies()?;

    let client = MenuClient::new();
    let server_client = client.clone();
    let server_ready = std::thread::spawn(move || server_client.ensure_server_running());

    server_ready
        .join()
        .map_err(|_| anyhow!("menu server thread panicked"))??;

    let store_dir = ensure_password_store_dir()?;
    let mut path: Vec<String> = Vec::new();

    loop {
        let entries = load_entries(&store_dir)?;
        let menu_items = build_browser_items(&entries, &path, true)?;

        let prompt = if path.is_empty() {
            "Pass Menu".to_string()
        } else {
            format!("Pass Menu / {}", path.join("/"))
        };

        let selected = client.choice(prompt, menu_items, false)?;
        if selected.is_empty() {
            if path.is_empty() {
                return Ok(1);
            }
            path.pop();
            continue;
        }

        let metadata = selected[0]
            .metadata
            .as_ref()
            .ok_or_else(|| anyhow!("Selected menu item missing metadata"))?;

        match metadata.get("kind").map(String::as_str) {
            Some("folder") => {
                let folder = metadata
                    .get("path")
                    .ok_or_else(|| anyhow!("Folder item missing path metadata"))?;
                path = path_segments(folder);
            }
            Some("entry") => {
                let key = metadata
                    .get("key")
                    .ok_or_else(|| anyhow!("Entry item missing key metadata"))?;
                let entry = resolve_entry_by_name(&entries, key, false)?;
                copy_primary_entry(&entry)?;
                record_frecency(&entry.display_name)?;
            }
            Some("add") => {
                run_add_menu(path_prefix(&path).as_deref())?;
            }
            Some("edit") => {
                run_edit_browser(path_prefix(&path).as_deref())?;
            }
            Some("back") => {
                if path.is_empty() {
                    return Ok(1);
                }
                path.pop();
            }
            Some("close") => return Ok(0),
            Some("menu") => {}
            other => bail!("Unknown menu item kind: {:?}", other),
        }
    }
}

pub(super) fn run_add_menu(current_prefix: Option<&str>) -> Result<()> {
    let items = build_add_menu_items();

    match FzfWrapper::builder()
        .header(Header::fancy("Pass Add"))
        .prompt("Create")
        .args(fzf_mocha_args())
        .responsive_layout()
        .select(items)?
    {
        FzfResult::Selected(item) => match item.action {
            AddMenuAction::AddPassword => {
                insert_password_entry_with_prefix(None, current_prefix)?;
            }
            AddMenuAction::GeneratePassword => {
                generate_password_entry_with_prefix(None, current_prefix, DEFAULT_PASSWORD_LENGTH)?;
            }
            AddMenuAction::AddOtp => {
                ensure_otp_dependency()?;
                insert_otp_entry_with_prefix(None, current_prefix)?;
            }
            AddMenuAction::Back => {}
        },
        _ => {}
    }

    Ok(())
}

pub(super) fn run_edit_browser(initial_prefix: Option<&str>) -> Result<()> {
    let store_dir = ensure_password_store_dir()?;
    let mut path = initial_prefix.map(path_segments).unwrap_or_default();
    let mut cursor = MenuCursor::new();

    loop {
        let mut entries = load_entries(&store_dir)?;
        sort_entries_by_frecency(&mut entries)?;
        let browser_items = build_local_browser_items(&entries, &path, false)?;

        let title = if path.is_empty() {
            "Edit Pass Entry".to_string()
        } else {
            format!("Edit Pass Entry / {}", path.join("/"))
        };

        let mut builder = FzfWrapper::builder()
            .header(Header::fancy(&title))
            .prompt("Select")
            .args(fzf_mocha_args())
            .responsive_layout();

        if let Some(index) = cursor.initial_index(&browser_items) {
            builder = builder.initial_index(index);
        }

        match builder.select(browser_items.clone())? {
            FzfResult::Selected(item) => {
                cursor.update(&item, &browser_items);
                match item.kind {
                    BrowserItemKind::Folder(folder) => path = path_segments(&folder),
                    BrowserItemKind::Entry(key) => {
                        let entry = resolve_entry_by_name(&entries, &key, false)?;
                        run_edit_action_menu(&entry)?;
                    }
                    BrowserItemKind::Back => {
                        if path.is_empty() {
                            return Ok(());
                        }
                        path.pop();
                    }
                    BrowserItemKind::Close => return Ok(()),
                    BrowserItemKind::Add => run_add_menu(path_prefix(&path).as_deref())?,
                    BrowserItemKind::Edit | BrowserItemKind::Menu => {}
                }
            }
            FzfResult::Cancelled => {
                if path.is_empty() {
                    return Ok(());
                }
                path.pop();
            }
            _ => return Ok(()),
        }
    }
}

pub(super) fn run_edit_action_menu(entry: &PassEntry) -> Result<()> {
    let mut cursor = MenuCursor::new();

    loop {
        let refreshed_entries = load_entries(&ensure_password_store_dir()?)?;
        let current_entry = resolve_entry_by_name(&refreshed_entries, &entry.display_name, false)
            .or_else(|_| {
                resolve_entry_by_name(
                    &refreshed_entries,
                    entry.secret_key.as_deref().unwrap_or(&entry.display_name),
                    false,
                )
            });

        let current_entry = match current_entry {
            Ok(entry) => entry,
            Err(_) => return Ok(()),
        };

        let items = build_edit_action_items(&current_entry);
        let mut builder = FzfWrapper::builder()
            .header(Header::fancy(&format!(
                "Edit: {}",
                current_entry.display_name
            )))
            .prompt("Action")
            .args(fzf_mocha_args())
            .responsive_layout();

        if let Some(index) = cursor.initial_index(&items) {
            builder = builder.initial_index(index);
        }

        match builder.select(items.clone())? {
            FzfResult::Selected(item) => {
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
                        export_entry_flow(Some(current_entry.display_name.clone()), None)?;
                    }
                    EditAction::Rename => {
                        rename_entry_interactive(&current_entry)?;
                    }
                    EditAction::EditPassword => {
                        let password = prompt_password_with_confirmation("New password")?;
                        let key = current_entry
                            .secret_key
                            .clone()
                            .unwrap_or(current_entry.display_name.clone());
                        insert_password(&key, &password)?;
                        maybe_notify(
                            "Pass",
                            &format!("Updated password for {}", current_entry.display_name),
                        );
                    }
                    EditAction::GeneratePassword => {
                        let key = current_entry
                            .secret_key
                            .clone()
                            .unwrap_or(current_entry.display_name.clone());
                        generate_password_entry(Some(key), DEFAULT_PASSWORD_LENGTH)?;
                    }
                    EditAction::EditOtp => {
                        ensure_otp_dependency()?;
                        upsert_otp_entry_interactive(&current_entry)?;
                    }
                    EditAction::Delete => {
                        delete_entry_flow(Some(current_entry.display_name.clone()))?;
                        return Ok(());
                    }
                    EditAction::Back => return Ok(()),
                }
            }
            FzfResult::Cancelled => return Ok(()),
            _ => return Ok(()),
        }
    }
}

fn build_add_menu_items() -> Vec<AddMenuItem> {
    vec![
        AddMenuItem {
            key: "add-password",
            display: format!(
                "{} Add Password",
                format_icon_colored(NerdFont::Plus, colors::GREEN)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Plus, "Add Password")
                .text("Create a password entry in the pass store.")
                .blank()
                .bullet("Prompts for a name")
                .bullet("Prompts for password + confirmation")
                .build(),
            action: AddMenuAction::AddPassword,
        },
        AddMenuItem {
            key: "generate-password",
            display: format!(
                "{} Generate Password",
                format_icon_colored(NerdFont::Refresh, colors::TEAL)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Refresh, "Generate Password")
                .text("Generate a 20-character password into a pass entry.")
                .blank()
                .bullet("Prompts for a name")
                .bullet("Uses `pass generate -f`")
                .build(),
            action: AddMenuAction::GeneratePassword,
        },
        AddMenuItem {
            key: "add-otp",
            display: format!(
                "{} Add OTP",
                format_icon_colored(NerdFont::Clock, colors::SAPPHIRE)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Clock, "Add OTP")
                .text("Create or replace an OTP companion using pass-otp.")
                .blank()
                .bullet("Prompts for a name")
                .bullet("Accepts only `otpauth://` URIs")
                .build(),
            action: AddMenuAction::AddOtp,
        },
        AddMenuItem {
            key: "back",
            display: format!("{} Back", format_back_icon()),
            preview: PreviewBuilder::new()
                .header(NerdFont::ArrowLeft, "Back")
                .text("Return to the pass browser.")
                .build(),
            action: AddMenuAction::Back,
        },
    ]
}

fn build_edit_action_items(entry: &PassEntry) -> Vec<EditActionItem> {
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
            .text("Rename the password entry and its OTP companion together when present.")
            .build(),
        action: EditAction::Rename,
    });

    items.push(EditActionItem {
        key: "edit-password",
        display: format!(
            "{} {} Password",
            format_icon_colored(NerdFont::Key, colors::PEACH),
            if entry.has_secret() { "Edit" } else { "Create" }
        ),
        preview: PreviewBuilder::new()
            .header(
                NerdFont::Key,
                if entry.has_secret() {
                    "Edit Password"
                } else {
                    "Create Password"
                },
            )
            .text("Replace or create the password entry content.")
            .build(),
        action: EditAction::EditPassword,
    });

    items.push(EditActionItem {
        key: "generate-password",
        display: format!(
            "{} {} Password",
            format_icon_colored(NerdFont::Refresh, colors::SAPPHIRE),
            if entry.has_secret() {
                "Generate"
            } else {
                "Generate New"
            }
        ),
        preview: PreviewBuilder::new()
            .header(NerdFont::Refresh, "Generate Password")
            .text("Generate a fresh password into the password entry.")
            .build(),
        action: EditAction::GeneratePassword,
    });

    items.push(EditActionItem {
        key: "edit-otp",
        display: format!(
            "{} {} OTP",
            format_icon_colored(NerdFont::Clock, colors::TEAL),
            if entry.has_otp() { "Edit" } else { "Create" }
        ),
        preview: PreviewBuilder::new()
            .header(
                NerdFont::Clock,
                if entry.has_otp() {
                    "Edit OTP"
                } else {
                    "Create OTP"
                },
            )
            .text("Replace or create the OTP companion entry.")
            .build(),
        action: EditAction::EditOtp,
    });

    items.push(EditActionItem {
        key: "delete",
        display: format!(
            "{} Delete",
            format_icon_colored(NerdFont::Trash, colors::RED)
        ),
        preview: PreviewBuilder::new()
            .header(NerdFont::Trash, "Delete Entry")
            .text("Remove the password entry, OTP companion, or both with confirmation.")
            .build(),
        action: EditAction::Delete,
    });

    items.push(EditActionItem {
        key: "back",
        display: format!("{} Back", format_back_icon()),
        preview: PreviewBuilder::new()
            .header(NerdFont::ArrowLeft, "Back")
            .text("Return to the edit browser.")
            .build(),
        action: EditAction::Back,
    });

    items
}
