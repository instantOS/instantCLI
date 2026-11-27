mod instantmenu;
mod move_entry;

use anyhow::{Result, anyhow, bail};

use crate::menu::client::HostedMenuClient;
use crate::menu::instantmenu::InstantmenuBackend;
use crate::menu::protocol::{ChoiceOptions, InputOptions};
use crate::menu_utils::{
    FzfWrapper, HeaderBuilder, MenuCursor, MenuKey, MenuKeybind, MenuSelection, MenuSort,
    choose_sort, sort_keybind,
};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::browser::*;
use super::operations::*;
use super::sort::{PassSort, sort_entries};
use super::types::*;
use super::utils::*;

/// Actions bound to global keys on the quick access menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuickAccessBind {
    /// Open the hierarchical edit browser.
    Edit,
    /// Change the order of the entry list.
    Sort,
}

enum QuickAccessIntent {
    Primary(BrowserMenuItem),
    EditMode(Option<BrowserMenuItem>),
    Sort(Option<BrowserMenuItem>),
}

/// Actions bound to global keys in the edit browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditBrowserBind {
    /// Add a new entry next to the focused row.
    Add,
    /// Open the password store in LazyGit.
    Lazygit,
    /// Change the order of the entry list.
    Sort,
}

fn quick_access_keybinds() -> Result<Vec<MenuKeybind<QuickAccessBind>>> {
    Ok(vec![
        MenuKeybind::new(MenuKey::new("ctrl-e")?, "edit mode", QuickAccessBind::Edit),
        sort_keybind()?.map_action(QuickAccessBind::Sort),
    ])
}

fn edit_browser_keybinds() -> Result<Vec<MenuKeybind<EditBrowserBind>>> {
    Ok(vec![
        MenuKeybind::new(MenuKey::new("ctrl-a")?, "add entry", EditBrowserBind::Add),
        MenuKeybind::new(
            MenuKey::overriding("ctrl-g")?,
            "lazygit",
            EditBrowserBind::Lazygit,
        ),
        sort_keybind()?.map_action(EditBrowserBind::Sort),
    ])
}

/// The folder a new entry should be created in when the add key is pressed.
///
/// A focused folder receives the new entry directly. A focused entry adds it
/// to the entry's parent folder. Anything else (the Add/Back/Close rows, or no
/// focused row because the filter matched nothing) falls back to the folder
/// currently being browsed.
fn add_prefix_for_selection(
    item: Option<&BrowserMenuItem>,
    current_path: &[String],
) -> Option<String> {
    match item.map(|item| &item.kind) {
        Some(BrowserItemKind::Folder(folder)) => Some(folder.clone()),
        Some(BrowserItemKind::Entry(key)) => {
            let mut segments = path_segments(key);
            segments.pop();
            (!segments.is_empty()).then(|| segments.join("/"))
        }
        _ => path_prefix(current_path),
    }
}

/// Initial folder and selected row for the edit browser. Focusing an entry
/// opens its parent folder and selects that entry there.
fn edit_browser_start(
    initial_prefix: Option<&str>,
    focus_entry: Option<&str>,
) -> (Vec<String>, Option<String>) {
    if let Some(entry) = focus_entry {
        let mut path = path_segments(entry);
        path.pop();
        return (path, Some(format!("entry:{entry}")));
    }

    (initial_prefix.map(path_segments).unwrap_or_default(), None)
}

fn interpret_quick_access_selection(
    selection: MenuSelection<BrowserMenuItem, QuickAccessBind>,
) -> Option<QuickAccessIntent> {
    match selection.action {
        Some(QuickAccessBind::Edit) => Some(QuickAccessIntent::EditMode(
            selection.items.first().cloned(),
        )),
        Some(QuickAccessBind::Sort) => {
            Some(QuickAccessIntent::Sort(selection.items.first().cloned()))
        }
        None => selection.into_single().ok().map(QuickAccessIntent::Primary),
    }
}

pub(super) use interactive_pass_quick_access as interactive_pass_menu;

pub(super) fn interactive_pass_quick_access() -> Result<i32> {
    ensure_core_dependencies()?;

    let store_dir = ensure_password_store_dir()?;
    let mut cursor = MenuCursor::new();
    let mut sort = PassSort::quick_access_default();

    loop {
        let mut entries = load_entries(&store_dir)?;
        sort_entries(&mut entries, sort)?;
        let quick_access_items = build_quick_access_items(&entries);

        let mut builder = FzfWrapper::builder()
            .header(
                HeaderBuilder::new(NerdFont::Key, "Pass")
                    .field("Sort", sort.label())
                    .build(),
            )
            .prompt("Copy")
            .responsive_layout();

        if let Some(index) = cursor.initial_index(&quick_access_items) {
            builder = builder.initial_index(index);
        }

        let keybinds = quick_access_keybinds()?;

        match builder
            .items(quick_access_items.clone())
            .keybinds(&keybinds)
            .select()?
        {
            crate::menu_utils::DialogOutcome::Submitted(sel) => {
                // Ctrl-E and Ctrl-S are mode switches even when the filter
                // matches nothing; a plain submit still needs a focused row.
                let Some(intent) = interpret_quick_access_selection(sel) else {
                    continue;
                };

                let item = match intent {
                    QuickAccessIntent::EditMode(item) => {
                        let focus = item.as_ref().and_then(|item| match &item.kind {
                            BrowserItemKind::Entry(key) => Some(key.as_str()),
                            _ => None,
                        });
                        run_edit_browser(None, focus)?;
                        continue;
                    }
                    QuickAccessIntent::Sort(item) => {
                        if let Some(item) = item {
                            cursor.update(&item, &quick_access_items);
                        }
                        if let Some(chosen) = choose_sort(sort)? {
                            sort = chosen;
                        }
                        continue;
                    }
                    QuickAccessIntent::Primary(item) => item,
                };

                cursor.update(&item, &quick_access_items);
                match item.kind {
                    BrowserItemKind::Entry(key) => {
                        let entry = resolve_entry_by_name(&entries, &key, false)?;
                        copy_primary_entry(&entry)?;
                        record_frecency(&entry.display_name)?;
                        // Exit after copying so the user can paste immediately
                        // (OTP codes expire; reopening is one keybinding press).
                        return Ok(0);
                    }
                    BrowserItemKind::Menu => {
                        interactive_pass_tree_menu()?;
                    }
                    BrowserItemKind::Close => return Ok(0),
                    BrowserItemKind::Folder(_) | BrowserItemKind::Add | BrowserItemKind::Back => {}
                }
            }
            crate::menu_utils::DialogOutcome::Cancelled => return Ok(1),
        }
    }
}

/// Quick-access password picker rendered directly by Instant Menu.
///
/// This mirrors the terminal picker without requiring a TTY: entries keep the
/// same smart ranking, Enter copies the selected secret, and Ctrl-E (or the
/// management row) opens the hierarchical editor in a terminal window.
pub(super) fn interactive_pass_quick_access_instantmenu(debug: bool) -> Result<i32> {
    ensure_core_dependencies()?;

    let store_dir = ensure_password_store_dir()?;
    let mut entries = load_entries(&store_dir)?;
    sort_entries(&mut entries, PassSort::quick_access_default())?;
    let items = build_quick_access_items(&entries);
    let keybinds = instantmenu::keybinds()?;

    let Some(selection) = instantmenu::select(&items, &keybinds)? else {
        return Ok(1);
    };

    if matches!(selection.action, Some(instantmenu::PassMenuAction::Edit)) {
        crate::common::terminal::launch_menu_in_terminal("pass", "Pass", &[], debug)?;
        return Ok(0);
    }

    let Some(item) = selection.items.into_iter().next() else {
        return Ok(1);
    };

    match item.kind {
        BrowserItemKind::Entry(key) => {
            let entry = resolve_entry_by_name(&entries, &key, false)?;
            if entry.should_offer_export() {
                if !export_entry_instantmenu(&entry)? {
                    return Ok(1);
                }
            } else {
                copy_primary_entry(&entry)?;
            }
            record_frecency(&entry.display_name)?;
            // Exit after copying so the user can paste immediately.
            Ok(0)
        }
        BrowserItemKind::Menu => {
            crate::common::terminal::launch_menu_in_terminal("pass", "Pass", &[], debug)?;
            Ok(0)
        }
        BrowserItemKind::Folder(_) | BrowserItemKind::Add | BrowserItemKind::Back => Ok(1),
        BrowserItemKind::Close => Ok(0),
    }
}

/// Export an entry that is too large or file-like for the clipboard using an
/// Instant Menu path prompt instead of the terminal-only fzf input dialog.
///
/// Returns `false` when the user cancels the path prompt.
fn export_entry_instantmenu(entry: &PassEntry) -> Result<bool> {
    let suggested = suggested_export_filename(&entry.display_name);
    let options = InputOptions::text_with_initial("Export path", suggested)
        .with_placeholder("Destination for the decrypted entry");
    let path = match InstantmenuBackend::input(&options)? {
        crate::menu_utils::DialogOutcome::Submitted(path) => path.trim().to_string(),
        crate::menu_utils::DialogOutcome::Cancelled => return Ok(false),
    };
    if path.is_empty() {
        bail!("Export path cannot be empty");
    }
    export_entry(entry, Some(path))?;
    Ok(true)
}

pub(super) use interactive_pass_quick_access_instantmenu as interactive_pass_menu_instantmenu;

pub(super) use interactive_pass_quick_access_server as interactive_pass_menu_server;

pub(super) fn interactive_pass_quick_access_server() -> Result<i32> {
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

        let metadata = selected.items[0]
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
                // Exit after copying so the user can paste immediately.
                return Ok(0);
            }
            Some("menu") => {
                crate::common::terminal::launch_menu_in_terminal("pass", "Pass", &[], false)?;
                return Ok(0);
            }
            Some("close") => return Ok(0),
            Some("folder" | "add" | "edit" | "back") => {}
            other => bail!("Unknown menu item kind: {:?}", other),
        }
    }
}

pub(super) fn interactive_pass_tree_menu() -> Result<i32> {
    run_edit_browser(None, None)?;
    Ok(0)
}

pub(super) fn run_add_menu(current_prefix: Option<&str>) -> Result<()> {
    let clipboard_otp = clipboard_otp_uri();
    let items = build_add_menu_items(clipboard_otp.is_some());

    let header = HeaderBuilder::new(NerdFont::Plus, "Add Password Store Entry")
        .subtitle("Create a new password, generated secret, OTP key, or file")
        .field("Destination", add_menu_destination(current_prefix))
        .build();

    if let crate::menu_utils::DialogOutcome::Submitted(item) = FzfWrapper::builder()
        .header(header)
        .prompt("Create")
        .responsive_layout()
        .items(items)
        .padded()
        .select_one()?
    {
        match item.action {
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
            AddMenuAction::AddOtpFromClipboard => {
                let Some(otp_uri) = clipboard_otp.as_deref() else {
                    bail!("The clipboard no longer contains a valid OTP URI");
                };
                ensure_otp_dependency()?;
                insert_otp_entry_from_clipboard_with_prefix(current_prefix, otp_uri)?;
            }
            AddMenuAction::AddFile => {
                insert_file_entry_with_prefix(current_prefix)?;
            }
            AddMenuAction::Back => {}
        }
    }

    Ok(())
}

pub(crate) fn run_edit_browser(
    initial_prefix: Option<&str>,
    focus_entry: Option<&str>,
) -> Result<()> {
    let store_dir = ensure_password_store_dir()?;
    let (mut path, initial_key) = edit_browser_start(initial_prefix, focus_entry);
    let mut cursor = MenuCursor::new();
    if let Some(key) = initial_key {
        cursor.update_from_key(&key);
    }
    let mut sort = PassSort::edit_browser_default();

    loop {
        let mut entries = load_entries(&store_dir)?;
        sort_entries(&mut entries, sort)?;
        let browser_items = build_browser_menu_items(&entries, &path)?;

        let title = if path.is_empty() {
            "Edit Passwords".to_string()
        } else {
            format!("Edit Passwords / {}", path.join("/"))
        };

        let mut builder = FzfWrapper::builder()
            .header(
                HeaderBuilder::new(NerdFont::Edit, &title)
                    .field("Sort", sort.label())
                    .build(),
            )
            .prompt("Select")
            .responsive_layout();

        if let Some(index) = cursor.initial_index(&browser_items) {
            builder = builder.initial_index(index);
        }

        let keybinds = edit_browser_keybinds()?;

        match builder
            .items(browser_items.clone())
            .keybinds(&keybinds)
            .select()?
        {
            crate::menu_utils::DialogOutcome::Submitted(selection) => {
                let action = selection.action;
                let item = selection.items.into_iter().next();

                if let Some(item) = &item {
                    cursor.update(item, &browser_items);
                }

                match action {
                    Some(EditBrowserBind::Add) => {
                        let prefix = add_prefix_for_selection(item.as_ref(), &path);
                        run_add_menu(prefix.as_deref())?;
                        continue;
                    }
                    Some(EditBrowserBind::Lazygit) => {
                        open_store_lazygit(&store_dir)?;
                        continue;
                    }
                    Some(EditBrowserBind::Sort) => {
                        if let Some(chosen) = choose_sort(sort)? {
                            sort = chosen;
                        }
                        continue;
                    }
                    None => {}
                }

                let Some(item) = item else {
                    continue;
                };

                match item.kind {
                    BrowserItemKind::Folder(folder) => path = path_segments(&folder),
                    BrowserItemKind::Entry(key) => {
                        let entry = resolve_entry_by_name(&entries, &key, false)?;
                        if let Some(moved_key) = run_edit_action_menu(&entry)? {
                            let (destination, focus) = edit_browser_start(None, Some(&moved_key));
                            path = destination;
                            if let Some(focus) = focus {
                                cursor.update_from_key(&focus);
                            }
                        }
                    }
                    BrowserItemKind::Back => {
                        if path.is_empty() {
                            return Ok(());
                        }
                        path.pop();
                    }
                    BrowserItemKind::Close => return Ok(()),
                    BrowserItemKind::Add => run_add_menu(path_prefix(&path).as_deref())?,
                    BrowserItemKind::Menu => {}
                }
            }
            crate::menu_utils::DialogOutcome::Cancelled => {
                if path.is_empty() {
                    return Ok(());
                }
                path.pop();
            }
        }
    }
}

fn run_edit_action_menu(entry: &PassEntry) -> Result<Option<String>> {
    let mut cursor = MenuCursor::new();
    let store_dir = ensure_password_store_dir()?;

    loop {
        let refreshed_entries = load_entries(&store_dir)?;
        let current_entry = resolve_entry_by_name(&refreshed_entries, &entry.display_name, false);

        let current_entry = match current_entry {
            Ok(entry) => entry,
            Err(_) => return Ok(None),
        };

        let items = build_edit_action_items(&current_entry);
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
                        export_entry_flow(Some(current_entry.display_name.clone()), None)?;
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
                        edit_password_entry(&current_entry)?;
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
                        return Ok(None);
                    }
                    EditAction::Back => return Ok(None),
                }
            }
            crate::menu_utils::DialogOutcome::Cancelled => return Ok(None),
        }
    }
}

fn add_menu_destination(current_prefix: Option<&str>) -> String {
    current_prefix
        .map(|prefix| format!("/{prefix}"))
        .unwrap_or_else(|| "/".to_string())
}

fn build_add_menu_items(clipboard_otp_available: bool) -> Vec<AddMenuItem> {
    let mut items = vec![
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
                .text("Create a standalone OTP entry using pass-otp.")
                .blank()
                .bullet("Prompts for a name")
                .bullet("Accepts only `otpauth://` URIs")
                .build(),
            action: AddMenuAction::AddOtp,
        },
    ];

    if clipboard_otp_available {
        items.push(AddMenuItem {
            key: "add-otp-from-clipboard",
            display: format!(
                "{} Add OTP from Clipboard",
                format_icon_colored(NerdFont::Clipboard, colors::MAUVE)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Clipboard, "Add OTP from Clipboard")
                .text("Use the OTP URI currently stored on your clipboard.")
                .blank()
                .bullet("The entry name is prefilled from the issuer")
                .bullet("The clipboard URI is not shown in this preview")
                .build(),
            action: AddMenuAction::AddOtpFromClipboard,
        });
    }

    items.extend([
        AddMenuItem {
            key: "add-file",
            display: format!(
                "{} Add File",
                format_icon_colored(NerdFont::File, colors::LAVENDER)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::File, "Add File")
                .text("Store a file in the pass store.")
                .blank()
                .bullet("Opens the Yazi file picker")
                .bullet("Prompts for a name and suggests the source filename")
                .bullet("Adds a `.file` suffix so the entry exports instead of copying")
                .build(),
            action: AddMenuAction::AddFile,
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
    ]);

    items
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
                .text("Open the entry in `pass edit` using `nvim --clean`.")
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
            .text("Return to the edit browser.")
            .build(),
        action: EditAction::Back,
    });

    items
}

#[cfg(test)]
mod keybind_tests {
    use super::*;

    fn entry_item(name: &str) -> BrowserMenuItem {
        BrowserMenuItem {
            key: format!("entry:{name}"),
            display: name.to_string(),
            preview: crate::ui::preview::FzfPreview::None,
            kind: BrowserItemKind::Entry(name.to_string()),
        }
    }

    fn folder_item(path: &str) -> BrowserMenuItem {
        BrowserMenuItem {
            key: format!("folder:{path}"),
            display: path.to_string(),
            preview: crate::ui::preview::FzfPreview::None,
            kind: BrowserItemKind::Folder(path.to_string()),
        }
    }

    #[test]
    fn enter_keeps_the_primary_copy_action() {
        let item = entry_item("mail/work");
        let interpreted = interpret_quick_access_selection(MenuSelection {
            items: vec![item.clone()],
            action: None,
        });

        assert!(matches!(
            interpreted,
            Some(QuickAccessIntent::Primary(selected)) if selected.key == item.key
        ));
    }

    #[test]
    fn edit_keybind_carries_the_focused_entry_into_edit_mode() {
        let item = entry_item("mail/work");
        let interpreted = interpret_quick_access_selection(MenuSelection {
            items: vec![item.clone()],
            action: Some(QuickAccessBind::Edit),
        });

        assert!(matches!(
            interpreted,
            Some(QuickAccessIntent::EditMode(Some(selected))) if selected.key == item.key
        ));
    }

    #[test]
    fn edit_keybind_enters_edit_mode_without_a_match() {
        let interpreted = interpret_quick_access_selection(MenuSelection::<BrowserMenuItem, _> {
            items: Vec::new(),
            action: Some(QuickAccessBind::Edit),
        });

        assert!(matches!(
            interpreted,
            Some(QuickAccessIntent::EditMode(None))
        ));
    }

    #[test]
    fn edit_focus_starts_in_the_entries_parent_folder() {
        let (path, key) = edit_browser_start(None, Some("mail/work/github"));

        assert_eq!(path.join("/"), "mail/work");
        assert_eq!(key.as_deref(), Some("entry:mail/work/github"));
    }

    #[test]
    fn edit_without_focus_uses_the_initial_prefix() {
        let (path, key) = edit_browser_start(Some("mail/work"), None);

        assert_eq!(path.join("/"), "mail/work");
        assert!(key.is_none());
    }

    #[test]
    fn sort_keybind_carries_the_focused_entry() {
        let item = entry_item("mail/work");
        let interpreted = interpret_quick_access_selection(MenuSelection {
            items: vec![item.clone()],
            action: Some(QuickAccessBind::Sort),
        });

        assert!(matches!(
            interpreted,
            Some(QuickAccessIntent::Sort(Some(selected))) if selected.key == item.key
        ));
    }

    #[test]
    fn sort_keybind_without_a_match_still_opens_the_chooser() {
        let interpreted = interpret_quick_access_selection(MenuSelection::<BrowserMenuItem, _> {
            items: Vec::new(),
            action: Some(QuickAccessBind::Sort),
        });

        assert!(matches!(interpreted, Some(QuickAccessIntent::Sort(None))));
    }

    #[test]
    fn edit_is_bound_to_ctrl_e() {
        let binds = quick_access_keybinds().unwrap();
        let bind = binds
            .iter()
            .find(|bind| bind.key.as_str() == "ctrl-e")
            .expect("quick access should bind ctrl-e");

        assert_eq!(bind.label, "edit mode");
        assert_eq!(bind.action, QuickAccessBind::Edit);
    }

    #[test]
    fn sort_is_bound_to_ctrl_s_in_quick_access() {
        let binds = quick_access_keybinds().unwrap();
        let bind = binds
            .iter()
            .find(|bind| bind.key.as_str() == "ctrl-s")
            .expect("quick access should bind ctrl-s");

        assert_eq!(bind.label, "sort");
        assert_eq!(bind.action, QuickAccessBind::Sort);
    }

    #[test]
    fn add_is_bound_to_ctrl_a() {
        let binds = edit_browser_keybinds().unwrap();
        let bind = binds
            .iter()
            .find(|bind| bind.key.as_str() == "ctrl-a")
            .expect("edit browser should bind ctrl-a");

        assert_eq!(bind.label, "add entry");
        assert_eq!(bind.action, EditBrowserBind::Add);
    }

    #[test]
    fn lazygit_is_bound_to_ctrl_g() {
        let binds = edit_browser_keybinds().unwrap();
        let bind = binds
            .iter()
            .find(|bind| bind.key.as_str() == "ctrl-g")
            .expect("edit browser should bind ctrl-g");

        assert_eq!(bind.label, "lazygit");
        assert_eq!(bind.action, EditBrowserBind::Lazygit);
    }

    #[test]
    fn sort_is_bound_to_ctrl_s_in_the_edit_browser() {
        let binds = edit_browser_keybinds().unwrap();
        let bind = binds
            .iter()
            .find(|bind| bind.key.as_str() == "ctrl-s")
            .expect("edit browser should bind ctrl-s");

        assert_eq!(bind.label, "sort");
        assert_eq!(bind.action, EditBrowserBind::Sort);
    }

    #[test]
    fn add_on_a_folder_uses_that_folder() {
        let item = folder_item("mail/work");
        let prefix = add_prefix_for_selection(Some(&item), &[]);

        assert_eq!(prefix.as_deref(), Some("mail/work"));
    }

    #[test]
    fn add_on_an_entry_uses_the_entrys_parent_folder() {
        let item = entry_item("mail/work/github");
        let prefix = add_prefix_for_selection(Some(&item), &[]);

        assert_eq!(prefix.as_deref(), Some("mail/work"));
    }

    #[test]
    fn add_on_a_root_entry_uses_the_store_root() {
        let item = entry_item("github");
        let prefix = add_prefix_for_selection(Some(&item), &[]);

        assert_eq!(prefix, None);
    }

    #[test]
    fn add_without_a_focused_row_uses_the_current_folder() {
        let current = ["mail".to_string(), "work".to_string()];
        let prefix = add_prefix_for_selection(None, &current);

        assert_eq!(prefix.as_deref(), Some("mail/work"));
    }

    #[test]
    fn add_on_a_non_entry_row_uses_the_current_folder() {
        let item = BrowserMenuItem {
            key: "back".to_string(),
            display: "Back".to_string(),
            preview: crate::ui::preview::FzfPreview::None,
            kind: BrowserItemKind::Back,
        };
        let current = ["mail".to_string()];
        let prefix = add_prefix_for_selection(Some(&item), &current);

        assert_eq!(prefix.as_deref(), Some("mail"));
    }
}

#[cfg(test)]
mod add_menu_tests {
    use super::*;

    #[test]
    fn destination_defaults_to_the_store_root() {
        assert_eq!(add_menu_destination(None), "/");
    }

    #[test]
    fn destination_shows_the_current_folder() {
        assert_eq!(add_menu_destination(Some("mail/work")), "/mail/work");
    }

    #[test]
    fn add_menu_offers_file_entries() {
        let items = build_add_menu_items(false);

        assert!(
            items
                .iter()
                .any(|item| matches!(item.action, AddMenuAction::AddFile))
        );
    }

    #[test]
    fn add_menu_only_offers_clipboard_otp_when_available() {
        let without_clipboard = build_add_menu_items(false);
        assert!(
            without_clipboard
                .iter()
                .all(|item| !matches!(item.action, AddMenuAction::AddOtpFromClipboard))
        );

        let with_clipboard = build_add_menu_items(true);
        assert!(
            with_clipboard
                .iter()
                .any(|item| matches!(item.action, AddMenuAction::AddOtpFromClipboard))
        );
    }
}
