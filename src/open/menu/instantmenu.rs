//! Compact rendering only; discovery and action handling live in the shared menu.
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use crossbeam_channel::Receiver;

use super::{OpenAction, OpenEntry};
use crate::menu::instantmenu::choice_flags_with_width;
use crate::menu::protocol::{Binding, ChoiceOptions};
use crate::menu_utils::process::tracked_spawn;
use crate::menu_utils::{MenuItem, MenuKeybind, MenuSelection};

fn plain(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

fn render(item: &MenuItem<OpenEntry>, index: usize) -> String {
    match item {
        MenuItem::Separator(label) => format!("{{heading fade}} {}", plain(label)),
        MenuItem::Entry(OpenEntry::App { app, recent, .. }) => {
            let color = if app.is_default { "green " } else { "" };
            let name = app.name.as_deref().unwrap_or(&app.desktop_id);
            format!(
                "{{{color}icon=mdapplication value={index}}} {} ({}){}{}",
                plain(name),
                plain(&app.desktop_id),
                if app.is_default { " (current)" } else { "" },
                if *recent { " · recent" } else { "" },
            )
        }
        MenuItem::Entry(OpenEntry::Error(error)) => {
            format!("{{red icon=fawarning value={index}}} {}", plain(error))
        }
    }
}

pub(super) fn select(
    path: &Path,
    status: &str,
    items: Receiver<MenuItem<OpenEntry>>,
    keybinds: &[MenuKeybind<OpenAction>],
) -> Result<Option<MenuSelection<MenuItem<OpenEntry>, OpenAction>>> {
    let options = ChoiceOptions::new(format!("Open With · {}", plain(&path.to_string_lossy())))
        .with_bindings(
            keybinds
                .iter()
                .map(|bind| Binding {
                    key: bind.key.as_str().to_owned(),
                    label: bind.label.clone(),
                })
                .collect(),
        );
    let mut command = Command::new("instantmenu");
    command
        .args(choice_flags_with_width(&options, "640"))
        .arg("--reject-no-match")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = tracked_spawn(command).context("Failed to start Instant Menu")?;
    let stdin = child
        .inner_mut()
        .stdin
        .take()
        .context("Missing Instant Menu input")?;
    let status = status.to_owned();
    // Cancel the input pump when the window closes, even if MIME discovery is
    // blocked. Joining a producer waiting for discovery would delay Escape.
    let (cancel, cancelled) = crossbeam_channel::bounded::<()>(0);
    let pump = std::thread::Builder::new()
        .name("open-instantmenu".into())
        .spawn(move || -> Result<Vec<MenuItem<OpenEntry>>> {
            let mut writer = std::io::BufWriter::new(stdin);
            let mut retained = Vec::new();
            if !status.is_empty() {
                writeln!(writer, "{{heading green}} {}", plain(&status))?;
                writer.flush()?;
            }
            loop {
                crossbeam_channel::select! {
                    recv(cancelled) -> _ => break,
                    recv(items) -> item => {
                        let Ok(item) = item else { break };
                        let line = render(&item, retained.len());
                        retained.push(item);
                        if writeln!(writer, "{line}").and_then(|()| writer.flush()).is_err() {
                            break;
                        }
                    }
                }
            }
            Ok(retained)
        })
        .context("Failed to start Instant Menu item pump")?;
    let output = child.finish_with_output();
    drop(cancel);
    let retained = pump
        .join()
        .map_err(|_| anyhow::anyhow!("Instant Menu item pump panicked"))?;
    let output = output?;
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
    let retained = retained?;
    decode(
        &String::from_utf8(output.stdout).context("Invalid Instant Menu output")?,
        &retained,
        keybinds,
    )
}

fn decode(
    output: &str,
    items: &[MenuItem<OpenEntry>],
    keybinds: &[MenuKeybind<OpenAction>],
) -> Result<Option<MenuSelection<MenuItem<OpenEntry>, OpenAction>>> {
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
        let item = items
            .get(index)
            .context("Instant Menu returned an unknown selection")?;
        if matches!(item, MenuItem::Separator(_)) {
            bail!("Instant Menu submitted a heading");
        }
        selected.push(item.clone());
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

    #[test]
    fn actions_and_selection_ids_are_validated() -> Result<()> {
        let bindings = super::super::keybinds()?;
        let items = vec![
            MenuItem::separator("Heading"),
            MenuItem::entry(OpenEntry::Error("error".into())),
        ];
        assert!(matches!(
            decode("ctrl-d\n1\n", &items, &bindings)?
                .context("No selection")?
                .action,
            Some(OpenAction::SetDefault)
        ));
        assert_eq!(
            decode("\n1\n", &items, &bindings)?
                .context("No selection")?
                .items
                .len(),
            1
        );
        assert!(
            decode("ctrl-e\n", &items, &bindings)?
                .context("No action")?
                .items
                .is_empty()
        );
        for invalid in ["\n0\n", "\n2\n", "\nlabel\n", "unknown\n1\n", "\n1\n1\n"] {
            assert!(decode(invalid, &items, &bindings).is_err(), "{invalid:?}");
        }
        assert!(decode("", &items, &bindings)?.is_none());
        Ok(())
    }

    #[test]
    fn labels_cannot_inject_rows_or_markup() {
        assert_eq!(
            render(
                &MenuItem::entry(OpenEntry::Error("{heading}\nctrl-d".into())),
                3
            ),
            "{red icon=fawarning value=3} {heading} ctrl-d"
        );
    }
}
