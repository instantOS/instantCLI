use std::io::{ErrorKind, Write};
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

use crate::assist::utils::copy_to_clipboard;
use crate::common::display_server::DisplayServer;

// A flat UTF-8 blob avoids per-entry static pointers and lookup tables.
const CATALOG: &str = include_str!("emoji/catalog.tsv");

fn entries() -> impl Iterator<Item = (&'static str, &'static str)> {
    CATALOG
        .lines()
        .filter(|line| !line.starts_with("# "))
        .filter_map(|line| line.split_once('\t'))
}

fn menu_input() -> String {
    let mut input = String::new();
    for (emoji, name) in entries() {
        // Fully-qualified emoji sequences contain no markup delimiters. Names
        // live outside the attribute block, so they need no markup escaping.
        input.push_str(&format!("{{value=\"{emoji}\"}} {emoji} {name}\n"));
    }
    input
}

fn selected_emoji(output: &str) -> Result<Option<String>> {
    let mut selected = String::new();
    // Ctrl-Return can emit several selections before the menu exits.
    for value in output.lines() {
        anyhow::ensure!(
            entries().any(|(emoji, _)| emoji == value),
            "Emoji picker returned a value outside the emoji catalog"
        );
        selected.push_str(value);
    }
    Ok((!selected.is_empty()).then_some(selected))
}

fn pick_with_command(command: &mut Command) -> Result<Option<String>> {
    let mut child = command
        .args([
            "--prompt",
            "Emoji",
            "--placeholder",
            "Search emoji names",
            "--insensitive",
            "--lines",
            "12",
            "--width",
            "700",
            "--position",
            "center",
            "--line-height",
            "32",
            "--frecency-cache",
            "assist-emoji",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("Failed to launch instantmenu emoji picker")?;

    let write_result = child
        .stdin
        .take()
        .context("Missing instantmenu stdin")?
        .write_all(menu_input().as_bytes());
    // Always reap the child, including cancellation during streamed input.
    let output = child
        .wait_with_output()
        .context("Failed to wait for emoji picker")?;
    if let Err(error) = write_result
        && error.kind() != ErrorKind::BrokenPipe
    {
        return Err(error).context("Failed to write emoji catalog to instantmenu");
    }
    if !output.status.success() {
        // instantmenu uses exit 1 for Escape/outside-click cancellation.
        anyhow::ensure!(
            output.status.code() == Some(1),
            "Emoji picker failed: {}",
            output.status
        );
        return Ok(None);
    }
    let output = String::from_utf8(output.stdout).context("Emoji picker returned invalid UTF-8")?;
    selected_emoji(&output)
}

pub fn emoji_picker() -> Result<()> {
    let display_server = DisplayServer::detect();
    if let Some(emoji) = pick_with_command(&mut Command::new("instantmenu"))? {
        copy_to_clipboard(emoji.as_bytes(), &display_server)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_menu(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", script, "fake-instantmenu"]);
        command
    }

    #[test]
    fn subprocess_selection_reads_catalog_and_closes_stdin() {
        let directory = tempfile::tempdir().unwrap();
        let input_path = directory.path().join("input");
        let mut command = fake_menu("cat > \"$INPUT_PATH\"; printf '👩‍💻\\n❤️\\n'");
        command.env("INPUT_PATH", &input_path);
        assert_eq!(
            pick_with_command(&mut command).unwrap().as_deref(),
            Some("👩‍💻❤️")
        );
        assert_eq!(std::fs::read_to_string(input_path).unwrap(), menu_input());
    }

    #[test]
    fn subprocess_cancel_during_streaming_returns_no_selection() {
        assert_eq!(pick_with_command(&mut fake_menu("exit 1")).unwrap(), None);
        assert_eq!(
            pick_with_command(&mut fake_menu("cat >/dev/null; printf '😀\\n'; exit 1")).unwrap(),
            None
        );
    }

    #[test]
    fn subprocess_rejects_failure_and_invalid_output() {
        for script in [
            "exit 2",
            "cat >/dev/null; printf 'not an emoji\\n'",
            "cat >/dev/null; printf '\\377'",
        ] {
            assert!(pick_with_command(&mut fake_menu(script)).is_err());
        }
        assert!(pick_with_command(&mut Command::new("/nonexistent/instantmenu")).is_err());
    }

    #[test]
    fn catalog_is_complete_and_unique() {
        let entries: Vec<_> = entries().collect();
        assert_eq!(entries.len(), 3944);
        let mut unique = std::collections::HashSet::new();
        for (emoji, name) in entries {
            assert!(unique.insert(emoji));
            assert!(!emoji.is_empty() && !name.is_empty());
            assert!(
                !emoji
                    .chars()
                    .any(|c| c.is_whitespace() || matches!(c, '"' | '\\' | '{' | '}'))
            );
        }
        assert_eq!(
            CATALOG
                .lines()
                .filter(|line| !line.starts_with("# "))
                .count(),
            unique.len()
        );
    }

    #[test]
    fn menu_preserves_full_sequences_and_searchable_names() {
        let input = menu_input();
        assert!(input.contains("{value=\"👩‍💻\"} 👩‍💻 woman technologist\n"));
        assert!(input.contains("{value=\"❤️\"} ❤️ red heart\n"));
    }

    #[test]
    fn selection_preserves_sequences_and_combines_multiple_values() {
        assert_eq!(selected_emoji("👩‍💻\n❤️\n").unwrap().as_deref(), Some("👩‍💻❤️"));
        assert_eq!(selected_emoji("").unwrap(), None);
        for invalid in ["hello\n", "😀 grinning face\n", "\n", "😀\ninvalid\n"] {
            assert!(selected_emoji(invalid).is_err());
        }
    }
}
