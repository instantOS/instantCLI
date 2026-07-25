use anyhow::Result;

use crate::assist::utils::copy_to_clipboard;
use crate::common::display_server::DisplayServer;
use crate::menu_utils::{FzfPreview, FzfResult, FzfSelectable, FzfWrapper, HeaderBuilder};
use crate::ui::catppuccin::{colors, format_icon_colored, hex_to_ansi_fg};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::history::ClipEntry;

const RESET: &str = "\x1b[0m";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipMenuItem(pub ClipEntry);

impl FzfSelectable for ClipMenuItem {
    fn fzf_display_text(&self) -> String {
        let summary = self.0.summary.replace('\t', " ");
        let color = hex_to_ansi_fg(colors::TEXT);
        format!(
            "{} {color}{summary}{RESET}",
            format_icon_colored(NerdFont::Clipboard, colors::BLUE)
        )
    }

    fn fzf_preview(&self) -> FzfPreview {
        if let Some(command) = self.0.preview_command() {
            return FzfPreview::Command(command);
        }
        let content = self.0.preview().unwrap_or_else(|error| error.to_string());
        let line_count = content.lines().count().max(1);
        let byte_count = content.len();
        let mut preview = PreviewBuilder::new()
            .header(NerdFont::Clipboard, "Clipboard entry")
            .field("ID", &self.0.id)
            .field("Size", &format!("{byte_count} bytes · {line_count} lines"))
            .blank()
            .separator()
            .blank();
        for line in content.lines() {
            preview = preview.raw(line);
        }
        preview.build()
    }

    fn fzf_key(&self) -> String {
        self.0.id.clone()
    }
}

pub fn run(entries: Vec<ClipEntry>) -> Result<()> {
    let items = entries.into_iter().map(ClipMenuItem).collect();
    let header = HeaderBuilder::new(NerdFont::Clipboard, "Clipboard History")
        .subtitle("Enter restore · Esc close")
        .build();
    match FzfWrapper::builder()
        .prompt("Clipboard")
        .header(header)
        .responsive_layout()
        .select(items)?
    {
        FzfResult::Selected(item) => copy_to_clipboard(&item.0.decode()?, &DisplayServer::detect()),
        FzfResult::Cancelled => Ok(()),
        FzfResult::Error(error) => Err(anyhow::anyhow!(error)),
        FzfResult::MultiSelected(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> ClipEntry {
        ClipEntry {
            id: "abc123".into(),
            summary: "hello world".into(),
            source: super::super::history::EntrySource::Memory(b"hello\nworld\n".to_vec()),
        }
    }

    #[test]
    fn menu_item_has_stable_key_and_full_preview() {
        let item = ClipMenuItem(entry());
        assert_eq!(item.fzf_key(), "abc123");
        assert!(item.fzf_display_text().contains("hello world"));
        let FzfPreview::Text(preview) = item.fzf_preview() else {
            panic!("expected text preview");
        };
        assert!(preview.contains("hello"));
        assert!(preview.contains("world"));
        assert!(preview.contains("2 lines"));
    }
}
