//! Compact native Instant Menu frontend for `ins pass --instantmenu`.
use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use super::super::browser::build_quick_access_items;
use super::super::dependencies::ensure_core_dependencies;
use super::super::names::suggested_export_filename;
use super::super::operations::{export_entry, resolve_entry_by_name};
use super::super::sort::{PassSort, sort_entries};
use super::super::store::{ensure_password_store_dir, load_entries, record_frecency};
use super::super::types::{BrowserItemKind, BrowserMenuItem, EntryKind, PassEntry};
use crate::menu::instantmenu::{InstantmenuBackend, choice_flags_with_width};
use crate::menu::protocol::{Binding, ChoiceOptions, InputOptions};
use crate::menu_utils::process::tracked_spawn;
use crate::menu_utils::{MenuKey, MenuKeybind, MenuSelection};

/// Actions bound to global keys in the compact picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PassMenuAction {
    /// Open the hierarchical tree view in a terminal.
    TreeView,
}

pub(super) fn keybinds() -> Result<Vec<MenuKeybind<PassMenuAction>>> {
    Ok(vec![MenuKeybind::new(
        MenuKey::new("ctrl-t")?,
        "tree view",
        PassMenuAction::TreeView,
    )])
}

pub(crate) fn run(debug: bool) -> Result<i32> {
    ensure_core_dependencies()?;

    let store_dir = ensure_password_store_dir()?;
    let mut entries = load_entries(&store_dir)?;
    sort_entries(&mut entries, PassSort::quick_access_default())?;
    let items = build_quick_access_items(&entries);
    let keybinds = keybinds()?;

    let Some(selection) = select(&items, &keybinds)? else {
        return Ok(1);
    };

    if matches!(selection.action, Some(PassMenuAction::TreeView)) {
        crate::common::terminal::launch_menu_in_terminal("pass", "Pass", &[], debug)?;
        return Ok(0);
    }

    let Some(item) = selection.items.into_iter().next() else {
        return Ok(1);
    };

    match item.kind {
        BrowserItemKind::Entry { key, .. } => {
            let entry = resolve_entry_by_name(&entries, &key, false)?;
            if entry.should_offer_export() {
                if !export_entry_prompt(&entry)? {
                    return Ok(1);
                }
                record_frecency(&entry.display_name)?;
            } else {
                super::copy_entry(&entries, &key)?;
            }
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
fn export_entry_prompt(entry: &PassEntry) -> Result<bool> {
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

fn plain(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

fn entry_style(kind: EntryKind) -> (&'static str, &'static str) {
    match kind {
        EntryKind::File => ("yellow", "fafile"),
        EntryKind::Otp => ("blue", "faclock"),
        EntryKind::Password => ("green", "fakey"),
    }
}

fn render(item: &BrowserMenuItem, index: usize) -> String {
    match &item.kind {
        BrowserItemKind::Entry { key, kind } => {
            let (color, icon) = entry_style(*kind);
            format!("{{{color} icon={icon} value={index}}} {}", plain(key))
        }
        BrowserItemKind::Menu => {
            format!("{{blue icon=fafolder value={index}}} Tree View")
        }
        BrowserItemKind::Folder(path) => {
            format!("{{blue icon=fafolder value={index}}} {}", plain(path))
        }
        BrowserItemKind::Add => format!("{{green icon=faplus value={index}}} Add"),
        BrowserItemKind::Back => {
            format!("{{fade icon=faarrowleft value={index}}} Back")
        }
        BrowserItemKind::Close => {
            format!("{{fade icon=fatimes value={index}}} Close")
        }
    }
}

fn bindings(keybinds: &[MenuKeybind<PassMenuAction>]) -> Vec<Binding> {
    keybinds
        .iter()
        .map(|bind| Binding {
            key: bind.key.as_str().to_owned(),
            label: bind.label.clone(),
        })
        .collect()
}

/// Show the compact picker and return the selected row or an action key.
pub(super) fn select(
    items: &[BrowserMenuItem],
    keybinds: &[MenuKeybind<PassMenuAction>],
) -> Result<Option<MenuSelection<BrowserMenuItem, PassMenuAction>>> {
    let options = ChoiceOptions::new("Pass").with_bindings(bindings(keybinds));
    let mut command = Command::new("instantmenu");
    command
        .args(choice_flags_with_width(&options, "640"))
        .arg("--placeholder")
        .arg("Search passwords")
        .arg("--reject-no-match")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = tracked_spawn(&mut command).context("Failed to start Instant Menu")?;
    let mut stdin = child
        .inner_mut()
        .stdin
        .take()
        .context("Missing Instant Menu input")?;
    {
        let mut writer = std::io::BufWriter::new(&mut stdin);
        for (index, item) in items.iter().enumerate() {
            // An early Escape closes the pipe; still wait for the status so it
            // is decoded as cancellation instead of a write error.
            if writeln!(writer, "{}", render(item, index)).is_err() {
                break;
            }
        }
        let _ = writer.flush();
    }
    drop(stdin);

    let output = child.finish_with_output()?;
    if !output.status.success() {
        // Instant Menu uses status 1 for both Escape and runtime failures.
        // Informational warnings on stderr do not turn Escape into an error.
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        let has_error = diagnostic.lines().any(|line| {
            let line = line.trim();
            !line.is_empty() && !line.starts_with("instantmenu: warning:")
        });
        if output.status.code() == Some(1) && !has_error {
            return Ok(None);
        }
        bail!(
            "Instant Menu failed ({}): {}",
            output.status,
            diagnostic.trim()
        );
    }

    decode(
        &String::from_utf8(output.stdout).context("Invalid Instant Menu output")?,
        items,
        keybinds,
    )
}

fn decode(
    output: &str,
    items: &[BrowserMenuItem],
    keybinds: &[MenuKeybind<PassMenuAction>],
) -> Result<Option<MenuSelection<BrowserMenuItem, PassMenuAction>>> {
    let mut lines = output.lines();
    let Some(key) = lines.next() else {
        return Ok(None);
    };
    let action = if key.is_empty() {
        None
    } else {
        Some(
            keybinds
                .iter()
                .find(|bind| bind.key.as_str() == key)
                .context("Instant Menu returned an unknown action")?
                .action,
        )
    };
    let mut selected = Vec::new();
    if let Some(value) = lines.next() {
        let index: usize = value
            .parse()
            .context("Instant Menu returned an invalid selection")?;
        selected.push(
            items
                .get(index)
                .context("Instant Menu returned an unknown selection")?
                .clone(),
        );
    }
    if lines.next().is_some() {
        bail!("Instant Menu returned multiple selections");
    }
    if action.is_none() && selected.is_empty() {
        return Ok(None);
    }
    Ok(Some(MenuSelection {
        items: selected,
        action,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::protocol::FzfPreview;

    fn item(key: &str, kind: BrowserItemKind) -> BrowserMenuItem {
        BrowserMenuItem {
            key: key.to_string(),
            display: key.to_string(),
            preview: FzfPreview::None,
            kind,
        }
    }

    fn entry(key: &str, kind: EntryKind) -> BrowserItemKind {
        BrowserItemKind::Entry {
            key: key.to_string(),
            kind,
        }
    }

    #[test]
    fn rows_use_kind_icons_and_stable_indexes() {
        assert_eq!(
            render(
                &item("entry:mail/work", entry("mail/work", EntryKind::Password)),
                0
            ),
            "{green icon=fakey value=0} mail/work"
        );
        assert_eq!(
            render(
                &item("entry:paypal.otp", entry("paypal.otp", EntryKind::Otp)),
                1
            ),
            "{blue icon=faclock value=1} paypal.otp"
        );
        assert_eq!(
            render(
                &item(
                    "entry:docs/contract.file",
                    entry("docs/contract.file", EntryKind::File)
                ),
                2
            ),
            "{yellow icon=fafile value=2} docs/contract.file"
        );
    }

    #[test]
    fn management_row_is_selectable() {
        assert_eq!(
            render(&item("menu", BrowserItemKind::Menu), 4),
            "{blue icon=fafolder value=4} Tree View"
        );
    }

    #[test]
    fn labels_cannot_inject_rows_or_markup() {
        assert_eq!(
            render(
                &item(
                    "entry:evil",
                    entry("{heading}\nctrl-t", EntryKind::Password)
                ),
                3
            ),
            "{green icon=fakey value=3} {heading} ctrl-t"
        );
    }

    #[test]
    fn enter_and_action_responses_are_decoded() -> Result<()> {
        let items = vec![
            item("entry:first", entry("first", EntryKind::Password)),
            item("entry:second", entry("second", EntryKind::Password)),
        ];
        let keybinds = keybinds()?;

        let selection = decode("\n1\n", &items, &keybinds)?.context("No selection")?;
        assert!(selection.action.is_none());
        assert_eq!(selection.items.len(), 1);
        assert_eq!(selection.items[0].key, "entry:second");

        let selection = decode("ctrl-t\n", &items, &keybinds)?.context("No action")?;
        assert_eq!(selection.action, Some(PassMenuAction::TreeView));
        assert!(selection.items.is_empty());

        let selection = decode("ctrl-t\n0\n", &items, &keybinds)?.context("No action")?;
        assert_eq!(selection.action, Some(PassMenuAction::TreeView));
        assert_eq!(selection.items[0].key, "entry:first");
        Ok(())
    }

    #[test]
    fn invalid_responses_are_rejected() {
        let items = vec![item("entry:first", entry("first", EntryKind::Password))];
        let keybinds = keybinds().unwrap();
        for invalid in [
            "\n1\n" as &str,
            "\n2\n",
            "\nlabel\n",
            "unknown\n1\n",
            "\n0\n0\n",
        ] {
            assert!(decode(invalid, &items, &keybinds).is_err(), "{invalid:?}");
        }
        assert!(decode("", &items, &keybinds).unwrap().is_none());
    }

    #[test]
    fn tree_view_is_bound_to_ctrl_t() {
        let binds = keybinds().unwrap();
        let bind = binds
            .iter()
            .find(|bind| bind.key.as_str() == "ctrl-t")
            .expect("compact picker should bind ctrl-t");

        assert_eq!(bind.label, "tree view");
        assert_eq!(bind.action, PassMenuAction::TreeView);
    }
}
