//! Compact native Instant Menu rendering for `ins pass --instantmenu`.
//!
//! Entry loading, sorting, and the copy actions stay in the shared quick-access
//! menu. This module only renders rows and decodes the Instant Menu response.
use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use super::super::types::{BrowserItemKind, BrowserMenuItem};
use crate::menu::instantmenu::choice_flags_with_width;
use crate::menu::protocol::{Binding, ChoiceOptions};
use crate::menu_utils::process::tracked_spawn;
use crate::menu_utils::{MenuKey, MenuKeybind, MenuSelection};

/// Actions bound to global keys in the compact picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PassMenuAction {
    /// Open the hierarchical management menu in a terminal.
    Edit,
}

pub(super) fn keybinds() -> Result<Vec<MenuKeybind<PassMenuAction>>> {
    Ok(vec![MenuKeybind::new(
        MenuKey::new("ctrl-e")?,
        "edit passwords",
        PassMenuAction::Edit,
    )])
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

fn entry_style(name: &str) -> (&'static str, &'static str) {
    if name.ends_with(".file") {
        ("yellow", "fafile")
    } else if name.ends_with(".otp") {
        ("blue", "faclock")
    } else {
        ("green", "fakey")
    }
}

fn render(item: &BrowserMenuItem, index: usize) -> String {
    match &item.kind {
        BrowserItemKind::Entry(name) => {
            let (color, icon) = entry_style(name);
            format!("{{{color} icon={icon} value={index}}} {}", plain(name))
        }
        BrowserItemKind::Menu => {
            format!("{{blue icon=faedit value={index}}} Edit Passwords")
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

    #[test]
    fn rows_use_kind_icons_and_stable_indexes() {
        assert_eq!(
            render(
                &item(
                    "entry:mail/work",
                    BrowserItemKind::Entry("mail/work".into())
                ),
                0
            ),
            "{green icon=fakey value=0} mail/work"
        );
        assert_eq!(
            render(
                &item(
                    "entry:paypal.otp",
                    BrowserItemKind::Entry("paypal.otp".into())
                ),
                1
            ),
            "{blue icon=faclock value=1} paypal.otp"
        );
        assert_eq!(
            render(
                &item(
                    "entry:docs/contract.file",
                    BrowserItemKind::Entry("docs/contract.file".into())
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
            "{blue icon=faedit value=4} Edit Passwords"
        );
    }

    #[test]
    fn labels_cannot_inject_rows_or_markup() {
        assert_eq!(
            render(
                &item(
                    "entry:evil",
                    BrowserItemKind::Entry("{heading}\nctrl-e".into())
                ),
                3
            ),
            "{green icon=fakey value=3} {heading} ctrl-e"
        );
    }

    #[test]
    fn enter_and_action_responses_are_decoded() -> Result<()> {
        let items = vec![
            item("entry:first", BrowserItemKind::Entry("first".into())),
            item("entry:second", BrowserItemKind::Entry("second".into())),
        ];
        let keybinds = keybinds()?;

        let selection = decode("\n1\n", &items, &keybinds)?.context("No selection")?;
        assert!(selection.action.is_none());
        assert_eq!(selection.items.len(), 1);
        assert_eq!(selection.items[0].key, "entry:second");

        let selection = decode("ctrl-e\n", &items, &keybinds)?.context("No action")?;
        assert_eq!(selection.action, Some(PassMenuAction::Edit));
        assert!(selection.items.is_empty());

        let selection = decode("ctrl-e\n0\n", &items, &keybinds)?.context("No action")?;
        assert_eq!(selection.action, Some(PassMenuAction::Edit));
        assert_eq!(selection.items[0].key, "entry:first");
        Ok(())
    }

    #[test]
    fn invalid_responses_are_rejected() {
        let items = vec![item("entry:first", BrowserItemKind::Entry("first".into()))];
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
    fn edit_is_bound_to_ctrl_e() {
        let binds = keybinds().unwrap();
        let bind = binds
            .iter()
            .find(|bind| bind.key.as_str() == "ctrl-e")
            .expect("compact picker should bind ctrl-e");

        assert_eq!(bind.label, "edit passwords");
        assert_eq!(bind.action, PassMenuAction::Edit);
    }
}
