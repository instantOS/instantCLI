use anyhow::Result;

use crate::menu_utils::{
    ConfirmResult, FzfSelectable, FzfWrapper, Header, MenuCursor, MenuPresentation,
};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::prelude::*;
use crate::ui::preview::{FzfPreview, PreviewBuilder};

use super::history::{self, ClipBackend, ClipEntry};
use super::service::{self, ClipServiceStatus};

#[derive(Clone, Debug)]
enum SettingsItem {
    Capture(ClipServiceStatus),
    ClearHistory { count: usize },
    Back,
}

impl FzfSelectable for SettingsItem {
    fn fzf_display_text(&self) -> String {
        match self {
            Self::Capture(status) => {
                let (icon, color, label) = if status.active {
                    (NerdFont::Stop, colors::GREEN, "running")
                } else {
                    (NerdFont::PlayCircle, colors::YELLOW, "stopped")
                };
                format!(
                    "{} Clipboard capture ({label})",
                    format_icon_colored(icon, color)
                )
            }
            Self::ClearHistory { count } => format!(
                "{} Clear history ({count})",
                format_icon_colored(NerdFont::Trash, colors::RED)
            ),
            Self::Back => format!("{} Back", format_back_icon()),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            Self::Capture(status) => {
                let (icon, color, state, action) = if status.active {
                    (
                        NerdFont::CheckCircle,
                        colors::GREEN,
                        "Running",
                        "Select to stop capture and disable it for future logins.",
                    )
                } else {
                    (
                        NerdFont::Warning,
                        colors::YELLOW,
                        "Stopped",
                        "Select to start capture now and on future graphical logins.",
                    )
                };
                PreviewBuilder::new()
                    .header(NerdFont::Clipboard, "Clipboard Capture")
                    .line(color, Some(icon), state)
                    .field("Backend", status.backend.name())
                    .field("Installed", yes_no(status.installed))
                    .field("Starts on login", yes_no(status.enabled))
                    .separator()
                    .text(action)
                    .blank()
                    .text("Stopping capture does not erase existing history.")
                    .build()
            }
            Self::ClearHistory { count } => PreviewBuilder::new()
                .header(NerdFont::Trash, "Clear Clipboard History")
                .field("Entries", &count.to_string())
                .line(
                    colors::RED,
                    Some(NerdFont::Warning),
                    "This cannot be undone.",
                )
                .separator()
                .text(if *count == 0 {
                    "There is no clipboard history to clear."
                } else {
                    "Select to review the affected entries before clearing."
                })
                .build(),
            Self::Back => PreviewBuilder::new()
                .header(NerdFont::ArrowLeft, "Go Back")
                .text("Return to clipboard history.")
                .build(),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            Self::Capture(_) => "__capture__".to_string(),
            Self::ClearHistory { .. } => "__clear__".to_string(),
            Self::Back => "__back__".to_string(),
        }
    }

    fn fzf_is_selectable(&self) -> bool {
        !matches!(self, Self::ClearHistory { count: 0 })
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
        let items = vec![
            SettingsItem::Capture(status),
            SettingsItem::ClearHistory {
                count: entries.len(),
            },
            SettingsItem::Back,
        ];
        let initial_index = cursor.initial_index(&items);
        let crate::menu_utils::DialogOutcome::Submitted(selection) = FzfWrapper::menu()
            .cursor(initial_index)
            .presentation(MenuPresentation::Padded)
            .select_one(items.clone())?
        else {
            return Ok(());
        };
        cursor.update(&selection, &items);

        match selection {
            SettingsItem::Capture(status) if status.active => service::disable(backend)?,
            SettingsItem::Capture(_) => {
                service::enable(backend)?;
            }
            SettingsItem::ClearHistory { .. } => {
                if confirm_clear(&entries)? {
                    let count = history::clear(backend)?;
                    emit(
                        Level::Success,
                        "clip.cleared",
                        &format!("Cleared {count} clipboard entries."),
                        Some(serde_json::json!({ "deleted": count })),
                    );
                }
            }
            SettingsItem::Back => return Ok(()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClearAction {
    Clear,
    Cancel,
}

#[derive(Clone)]
struct ClearReview {
    action: ClearAction,
    count: usize,
    examples: Vec<String>,
}

impl FzfSelectable for ClearReview {
    fn fzf_display_text(&self) -> String {
        match self.action {
            ClearAction::Clear => format!(
                "{} Clear {} entries",
                format_icon_colored(NerdFont::Trash, colors::RED),
                self.count
            ),
            ClearAction::Cancel => format!("{} Keep history", format_back_icon()),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        if self.action == ClearAction::Cancel {
            return PreviewBuilder::new()
                .header(NerdFont::ArrowLeft, "Keep Clipboard History")
                .text("No clipboard entries will be removed.")
                .build();
        }

        let mut preview = PreviewBuilder::new()
            .header(NerdFont::Trash, "Clear Clipboard History")
            .field("Entries", &self.count.to_string())
            .line(
                colors::RED,
                Some(NerdFont::Warning),
                "This cannot be undone.",
            )
            .blank()
            .separator()
            .blank()
            .subtext("Examples that will be removed:");
        for example in &self.examples {
            preview = preview.bullet(example);
        }
        if self.count > self.examples.len() {
            preview = preview.subtext(&format!("…and {} more", self.count - self.examples.len()));
        }
        preview.build()
    }

    fn fzf_key(&self) -> String {
        match self.action {
            ClearAction::Clear => "__confirm_clear__".to_string(),
            ClearAction::Cancel => "__cancel_clear__".to_string(),
        }
    }
}

fn confirm_clear(entries: &[ClipEntry]) -> Result<bool> {
    if entries.is_empty() {
        return Ok(false);
    }
    let examples: Vec<String> = entries
        .iter()
        .take(8)
        .map(|entry| truncate(&entry.summary.replace('\t', " "), 56))
        .collect();
    let items = vec![
        ClearReview {
            action: ClearAction::Clear,
            count: entries.len(),
            examples: examples.clone(),
        },
        ClearReview {
            action: ClearAction::Cancel,
            count: entries.len(),
            examples,
        },
    ];
    let header = Header::default("Review clipboard history deletion");
    let crate::menu_utils::DialogOutcome::Submitted(selection) = FzfWrapper::menu()
        .initial_index(0)
        .header(header)
        .presentation(MenuPresentation::Padded)
        .select_one(items)?
    else {
        return Ok(false);
    };
    if selection.action != ClearAction::Clear {
        return Ok(false);
    }

    Ok(FzfWrapper::builder()
        .confirm(format!(
            "Permanently clear {} clipboard entries?",
            entries.len()
        ))
        .yes_text(format!("Clear {}", entries.len()))
        .no_text("Keep")
        .confirm_dialog()?
        == ConfirmResult::Yes)
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn truncate(value: &str, width: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= width {
        return trimmed.to_string();
    }
    let mut output: String = trimmed.chars().take(width.saturating_sub(1)).collect();
    output.push('…');
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu_utils::MockQueue;

    #[test]
    fn empty_clear_action_is_visible_but_disabled() {
        let item = SettingsItem::ClearHistory { count: 0 };
        assert!(!item.fzf_is_selectable());
        assert!(item.fzf_display_text().contains("(0)"));
    }

    #[test]
    fn clear_requires_review_and_confirmation() {
        let entries = vec![test_entry("one"), test_entry("two")];
        let _guard = MockQueue::new().select_index(0).confirm_yes().guard();
        assert!(confirm_clear(&entries).unwrap());
    }

    #[test]
    fn clear_can_be_cancelled_at_review() {
        let entries = vec![test_entry("one")];
        let _guard = MockQueue::new().select_index(1).guard();
        assert!(!confirm_clear(&entries).unwrap());
    }

    fn test_entry(summary: &str) -> ClipEntry {
        ClipEntry {
            id: summary.to_string(),
            summary: summary.to_string(),
            source: super::super::history::EntrySource::Memory(summary.as_bytes().to_vec()),
        }
    }
}
