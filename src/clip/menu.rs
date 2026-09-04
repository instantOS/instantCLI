use anyhow::Result;

use crate::assist::utils::copy_to_clipboard;
use crate::common::display_server::DisplayServer;
use crate::common::shell::current_exe_command;
use crate::menu_utils::{
    FzfPreview, FzfSelectable, FzfWrapper, HeaderBuilder, MenuCursor, MenuPresentation,
};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, hex_to_ansi_fg};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::history::{self, ClipBackend, ClipEntry};
use super::service;

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

#[derive(Clone, Debug, PartialEq, Eq)]
enum ClipMainItem {
    Entry(ClipMenuItem),
    EnableCapture(ClipBackend),
    Settings,
    Close,
}

impl FzfSelectable for ClipMainItem {
    fn fzf_display_text(&self) -> String {
        match self {
            Self::Entry(entry) => entry.fzf_display_text(),
            Self::EnableCapture(_) => format!(
                "{} Enable clipboard capture",
                format_icon_colored(NerdFont::PlayCircle, colors::GREEN)
            ),
            Self::Settings => format!(
                "{} Settings",
                format_icon_colored(NerdFont::Gear, colors::MAUVE)
            ),
            Self::Close => format!("{} Close", format_back_icon()),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            Self::Entry(entry) => entry.fzf_preview(),
            Self::EnableCapture(backend) => PreviewBuilder::new()
                .header(NerdFont::PlayCircle, "Enable Clipboard Capture")
                .field("Backend", backend.name())
                .text("Start clipboard capture now and automatically on future")
                .text("graphical logins.")
                .blank()
                .text("Existing clipboard history is preserved.")
                .build(),
            Self::Settings => PreviewBuilder::new()
                .header(NerdFont::Gear, "Clipboard Settings")
                .text("Manage background capture and clear clipboard history.")
                .build(),
            Self::Close => PreviewBuilder::new()
                .header(NerdFont::Cross, "Close")
                .text("Exit without changing the clipboard.")
                .build(),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            // The dynamic preview command receives this key as its entry ID.
            Self::Entry(entry) => entry.fzf_key(),
            Self::EnableCapture(_) => "__enable_capture__".to_string(),
            Self::Settings => "__settings__".to_string(),
            Self::Close => "__close__".to_string(),
        }
    }
}

pub fn run(backend: ClipBackend) -> Result<()> {
    let mut cursor = MenuCursor::new();
    loop {
        let status = service::status(backend);
        let entries = if status.installed {
            history::load(backend)?
        } else {
            Vec::new()
        };
        let entry_count = entries.len();
        let mut items = vec![ClipMainItem::Close, ClipMainItem::Settings];
        if !status.active {
            items.push(ClipMainItem::EnableCapture(backend));
        }
        items.extend(
            entries
                .into_iter()
                .map(|entry| ClipMainItem::Entry(ClipMenuItem(entry))),
        );

        let initial_index = cursor.initial_index(&items).or_else(|| {
            items
                .iter()
                .position(|item| matches!(item, ClipMainItem::Entry(_)))
                .or_else(|| {
                    items
                        .iter()
                        .position(|item| matches!(item, ClipMainItem::EnableCapture(_)))
                })
                .or_else(|| {
                    items
                        .iter()
                        .position(|item| matches!(item, ClipMainItem::Settings))
                })
        });
        let capture_label = if status.active {
            "capturing"
        } else {
            "stopped"
        };
        let capture_color = if status.active {
            colors::GREEN
        } else {
            colors::YELLOW
        };
        let header = HeaderBuilder::new(NerdFont::Clipboard, "Clipboard History")
            .status(
                NerdFont::Database,
                format!("{entry_count} entries"),
                colors::BLUE,
            )
            .status(NerdFont::Circle, capture_label, capture_color)
            .build();

        let crate::menu_utils::DialogOutcome::Submitted(selection) = FzfWrapper::menu()
            .cursor(initial_index)
            .header(header)
            .presentation(MenuPresentation::Padded)
            .select_one(items.clone())?
        else {
            return Ok(());
        };
        cursor.update(&selection, &items);
        match selection {
            ClipMainItem::Entry(entry) => {
                return copy_to_clipboard(&entry.0.decode()?, &DisplayServer::detect());
            }
            ClipMainItem::EnableCapture(backend) => {
                service::enable(backend)?;
            }
            ClipMainItem::Settings => super::settings::run(backend)?,
            ClipMainItem::Close => return Ok(()),
        }
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
    fn main_menu_preserves_entry_id_for_dynamic_preview() {
        let item = ClipMainItem::Entry(ClipMenuItem(entry()));
        assert_eq!(item.fzf_key(), "abc123");
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
