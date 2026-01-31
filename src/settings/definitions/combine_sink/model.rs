use crate::menu_utils::FzfSelectable;
use crate::ui::catppuccin::{colors, format_icon_colored};
use crate::ui::prelude::*;
use crate::ui::preview::{FzfPreview, PreviewBuilder};

/// Information about an audio sink device
#[derive(Debug, Clone)]
pub(super) struct SinkInfo {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) node_name: String,
    pub(super) description: String,
    pub(super) volume: Option<String>,
    pub(super) is_default: bool,
}

impl SinkInfo {
    fn display_label(&self) -> String {
        let default_tag = if self.is_default {
            format!(" [{}]", format_icon_colored(NerdFont::Star, colors::GREEN))
        } else {
            String::new()
        };
        format!("{}{}", self.description, default_tag)
    }
}

impl FzfSelectable for SinkInfo {
    fn fzf_display_text(&self) -> String {
        self.display_label()
    }

    fn fzf_key(&self) -> String {
        self.node_name.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        let mut builder = PreviewBuilder::new()
            .header(NerdFont::VolumeUp, &self.description)
            .line(
                colors::TEAL,
                Some(NerdFont::Hash),
                &format!("ID: {}", self.id),
            )
            .line(
                colors::TEAL,
                Some(NerdFont::Tag),
                &format!("Node: {}", self.node_name),
            );

        if let Some(vol) = &self.volume {
            builder = builder.line(
                colors::SKY,
                Some(NerdFont::VolumeUp),
                &format!("Volume: {}", vol),
            );
        }

        if self.is_default {
            builder = builder.line(
                colors::GREEN,
                Some(NerdFont::Star),
                "Currently set as default output",
            );
        }

        builder.build()
    }

    fn fzf_initial_checked_state(&self) -> bool {
        false
    }
}

/// Wrapper for SinkInfo with initial checked state for checklist
#[derive(Debug, Clone)]
pub(super) struct SinkChecklistItem {
    pub(super) sink: SinkInfo,
    initially_checked: bool,
}

impl SinkChecklistItem {
    pub(super) fn new(sink: SinkInfo, checked: bool) -> Self {
        Self {
            sink,
            initially_checked: checked,
        }
    }
}

impl FzfSelectable for SinkChecklistItem {
    fn fzf_display_text(&self) -> String {
        self.sink.fzf_display_text()
    }

    fn fzf_key(&self) -> String {
        self.sink.fzf_key()
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.sink.fzf_preview()
    }

    fn fzf_initial_checked_state(&self) -> bool {
        self.initially_checked
    }
}
