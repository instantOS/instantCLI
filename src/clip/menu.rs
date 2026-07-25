use anyhow::Result;

use crate::assist::utils::copy_to_clipboard;
use crate::common::display_server::DisplayServer;
use crate::common::shell::current_exe_command;
use crate::menu_utils::{FzfPreview, FzfResult, FzfSelectable, FzfWrapper, HeaderBuilder};
use crate::ui::catppuccin::{colors, format_icon_colored, hex_to_ansi_fg};
use crate::ui::nerd_font::NerdFont;

use super::history::ClipEntry;

const RESET: &str = "\x1b[0m";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipMenuItem(pub ClipEntry);

impl FzfSelectable for ClipMenuItem {
    fn fzf_display_text(&self) -> String {
        let summary = friendly_summary(&self.0.summary);
        let color = hex_to_ansi_fg(colors::TEXT);
        format!(
            "{} {color}{summary}{RESET}",
            format_icon_colored(NerdFont::Clipboard, colors::BLUE)
        )
    }

    fn fzf_preview(&self) -> FzfPreview {
        FzfPreview::Command(format!("{} clip preview \"$1\"", current_exe_command()))
    }

    fn fzf_key(&self) -> String {
        self.0.id.clone()
    }
}

fn friendly_summary(summary: &str) -> String {
    let summary = summary.replace('\t', " ");
    let Some(details) = summary
        .strip_prefix("[[ binary data ")
        .and_then(|value| value.strip_suffix(" ]]"))
    else {
        return summary;
    };
    let parts: Vec<_> = details.split_whitespace().collect();
    let kind = parts
        .iter()
        .find(|part| is_image_format(part))
        .map_or("Binary", |_| "Image");
    let format = parts
        .iter()
        .find(|part| is_image_format(part))
        .map(|part| part.to_ascii_uppercase());
    let dimensions = parts
        .iter()
        .find(|part| {
            part.split_once('x').is_some_and(|(width, height)| {
                width.chars().all(|c| c.is_ascii_digit())
                    && height.chars().all(|c| c.is_ascii_digit())
            })
        })
        .map(|part| part.replace('x', "×"));
    let size = (parts.len() >= 2).then(|| format!("{} {}", parts[0], parts[1]));

    std::iter::once(kind.to_string())
        .chain(format)
        .chain(dimensions)
        .chain(size)
        .collect::<Vec<_>>()
        .join(" · ")
}

fn is_image_format(value: &&str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tiff" | "svg"
    )
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
        let FzfPreview::Command(preview) = item.fzf_preview() else {
            panic!("expected command preview");
        };
        assert!(preview.contains("clip preview"));
    }

    #[test]
    fn binary_summaries_are_human_readable() {
        assert_eq!(
            friendly_summary("[[ binary data 2 KiB png ]]"),
            "Image · PNG · 2 KiB"
        );
        assert_eq!(
            friendly_summary("[[ binary data 76 KiB png 1249x364 ]]"),
            "Image · PNG · 1249×364 · 76 KiB"
        );
    }
}
