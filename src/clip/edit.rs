use std::fs;
use std::io::Write;
use std::process::Command;

use anyhow::{Context, Result, ensure};

use crate::assist::utils::copy_to_clipboard;
use crate::common::display_server::DisplayServer;
use crate::ui::{Level, emit};

use super::history::{self, ClipEntry};

pub(super) fn run(entry: &ClipEntry) -> Result<()> {
    ensure!(
        !entry.summary.starts_with("[[ binary data "),
        "Only plain text entries can be edited; this entry contains binary data"
    );
    let original = entry.decode()?;
    let mut editor = Command::new("nvim");
    // Keep clipboard text out of swap, backup, undo and ShaDa files. Preserve
    // the absence of a final newline when saving an entry copied from an app.
    editor.args([
        "-n",
        "-i",
        "NONE",
        "--cmd",
        "set nobackup nowritebackup noundofile",
        "-c",
        "setlocal nofixendofline",
        "--",
    ]);
    let Some(edited) = edit_text(&original, &mut editor)? else {
        return Ok(());
    };

    // Store explicitly so editing also works while background capture is stopped.
    history::store(&edited)?;
    copy_to_clipboard(&edited, &DisplayServer::detect())?;
    emit(
        Level::Success,
        "clip.edited",
        "Edited text copied to clipboard and saved in history; original entry preserved.",
        Some(serde_json::json!({ "original_id": entry.id })),
    );
    Ok(())
}

fn ensure_text(data: &[u8]) -> Result<()> {
    let text = std::str::from_utf8(data).context("Only UTF-8 plain text entries can be edited")?;
    ensure!(
        !text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')),
        "Only plain text entries can be edited; this entry contains binary data"
    );
    Ok(())
}

fn edit_text(original: &[u8], editor: &mut Command) -> Result<Option<Vec<u8>>> {
    ensure_text(original)?;
    let mut file = tempfile::Builder::new()
        .prefix("ins-clip-edit-")
        .suffix(".txt")
        .tempfile()
        .context("Failed to create clipboard editing file")?;
    file.write_all(original)
        .context("Failed to write clipboard editing file")?;
    let status = editor
        .arg(file.path())
        .status()
        .context("Failed to open clipboard editor; ensure Neovim (nvim) is installed")?;
    ensure!(
        status.success(),
        "Clipboard editor exited with {status}; changes were not applied"
    );
    // Read the path again: editors can replace the file during an atomic save.
    let edited = fs::read(file.path()).context("Failed to read edited clipboard text")?;
    ensure_text(&edited)?;
    if edited == original {
        return Ok(None);
    }
    ensure!(
        !edited.is_empty(),
        "Empty edits are not saved to clipboard history"
    );
    Ok(Some(edited))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", script, "clip-test-editor"]);
        command
    }

    #[test]
    fn accepts_unicode_text_and_rejects_binary() {
        assert!(ensure_text("hello\t世界\r\n".as_bytes()).is_ok());
        for bytes in [b"GIF89a\0".as_slice(), b"hello\0world", b"\xff\xfe"] {
            assert!(ensure_text(bytes).is_err());
        }
    }

    #[test]
    fn unchanged_text_is_not_applied() -> Result<()> {
        assert_eq!(
            edit_text(b"no final newline", &mut editor("test -f \"$1\""))?,
            None
        );
        Ok(())
    }

    #[test]
    fn reads_saved_text_even_when_editor_replaces_file() -> Result<()> {
        let edited = edit_text(
            b"original",
            &mut editor("printf 'edited\\ntext' > \"$1.new\" && mv \"$1.new\" \"$1\""),
        )?;
        assert_eq!(edited, Some(b"edited\ntext".to_vec()));
        Ok(())
    }

    #[test]
    fn failed_editor_and_invalid_edits_are_not_applied() {
        for script in [
            "printf changed > \"$1\"; exit 1",
            ": > \"$1\"",
            "printf '\\000' > \"$1\"",
        ] {
            assert!(edit_text(b"original", &mut editor(script)).is_err());
        }
    }
}
