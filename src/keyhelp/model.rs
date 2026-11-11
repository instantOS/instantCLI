use crate::menu_utils::FzfSelectable;
use crate::preview::{PreviewId, preview_command};
use crate::ui::catppuccin::{colors, format_bold, format_icon, format_with_color};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewWriter;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::GLOBAL_MODE;
use super::derive_keywords;

#[derive(Debug, Clone, Deserialize)]
pub(super) struct KeybindRowJson {
    pub(super) modifiers: String,
    pub(super) key: String,
    pub(super) action: String,
    pub(super) mode: Option<String>,
    pub(super) origin: String,
}

/// One keybinding as it appears in the fzf menu.
///
/// `fzf_preview()` returns a *command* (not text), so per-row preview work is
/// done by the `ins preview --id keyhelp` child process on highlight - see
/// [`KeybindPreviewPayload`] and `crate::preview::keyhelp::render_keyhelp_preview`.
#[derive(Debug, Clone)]
pub(super) struct KeybindRow {
    pub(super) modifiers: String,
    pub(super) key: String,
    pub(super) action: String,
    pub(super) mode: String,
    pub(super) origin: String,
    pub(super) keywords: Vec<&'static str>,
}

/// Serializable snapshot of a keybinding. The fzf `key` field carries this
/// JSON; the preview child deserializes it to render the binding's preview
/// without a second `instantwmctl keybinds` round-trip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeybindPreviewPayload {
    pub modifiers: String,
    pub key: String,
    pub action: String,
    pub mode: String,
    pub origin: String,
}

impl KeybindPreviewPayload {
    /// Render the chord as the user would type it: empty modifiers → just the
    /// key, otherwise `Modifiers + Key`.
    pub fn binding(&self) -> String {
        if self.modifiers.is_empty() {
            self.key.clone()
        } else {
            format!("{} + {}", self.modifiers, self.key)
        }
    }

    /// The individual actions of a `sequence [...]` binding, or `None` for a
    /// plain (non-sequence) action.
    pub fn sequence_steps(&self) -> Option<Vec<String>> {
        let inner = self.action.strip_prefix("sequence [")?.strip_suffix(']')?;
        Some(
            inner
                .split(',')
                .map(|step| step.trim().to_string())
                .filter(|step| !step.is_empty())
                .collect(),
        )
    }

    /// Human-readable origin label, shown only in the preview.
    pub fn origin_label(&self) -> &'static str {
        if self.origin == "user" {
            "your config"
        } else {
            "built-in default"
        }
    }

    /// Build the full preview text with prominent action description,
    /// structured Details section, and origin explanation.
    pub fn render_preview(&self, docs: &HashMap<String, ActionDoc>) -> String {
        let mut w = PreviewWriter::collect();

        // 1. Header with the keybinding chord
        w.header(NerdFont::Keyboard, &self.binding());

        // 2. Action description right under the header (primary information)
        if self.sequence_steps().is_some() {
            self.build_sequence_doc(docs, &mut w);
        } else if !docs.is_empty() {
            self.build_action_doc(docs, &mut w);
        }

        w.blank();
        w.separator();
        w.blank();

        // 3. Structured Details section
        w.title(colors::BLUE, "Details");
        w.field("Action", &self.action);

        let name = action_name(&self.action);
        if let Some(doc) = docs.get(name)
            && let Some(arg) = &doc.arg_example
        {
            w.field("Usage", &format!("{name} {arg}"));
        }

        if self.mode == GLOBAL_MODE {
            w.field("Mode", "global (all modes)");
        } else {
            w.field("Mode", &format!("{} mode", self.mode));
            w.line(
                colors::MAUVE,
                Some(NerdFont::Info),
                &format!("Only available when '{}' mode is active", self.mode),
            );
        }

        w.field("Origin", self.origin_label());
        w.blank();

        // 4. Clear footer explanation
        if self.origin == "user" {
            w.subtext(
                "Defined in ~/.config/instantwm/config.toml. Select 'Edit config' to change it.",
            );
        } else {
            w.subtext("Built-in default. You can override it in ~/.config/instantwm/config.toml.");
        }

        w.build_string()
    }

    fn build_action_doc(&self, docs: &HashMap<String, ActionDoc>, builder: &mut PreviewWriter) {
        let name = action_name(&self.action);
        match docs.get(name) {
            Some(doc) => {
                builder.line(colors::SKY, Some(NerdFont::Info), &doc.description);
            }
            None => {
                builder.subtext(&format!("Executes `{name}`."));
            }
        }
    }

    fn build_sequence_doc(&self, docs: &HashMap<String, ActionDoc>, builder: &mut PreviewWriter) {
        builder.title(colors::BLUE, "Sequence");
        match self.sequence_steps() {
            Some(steps) => {
                for step in &steps {
                    match docs.get(action_name(step)) {
                        Some(doc) => {
                            builder.bullet(&format!("{step}: {}", doc.description));
                        }
                        None => {
                            builder.bullet(step);
                        }
                    }
                }
            }
            None => {
                builder.subtext("Malformed sequence.");
            }
        }
    }
}

/// Documentation for one named WM action, sourced from
/// `instantwmctl --json action --list`.
#[derive(Debug, Clone)]
pub struct ActionDoc {
    pub description: String,
    pub arg_example: Option<String>,
}

impl KeybindRow {
    pub(super) fn from_json(j: KeybindRowJson) -> Self {
        let mode = j.mode.unwrap_or_else(|| GLOBAL_MODE.to_string());
        let keywords = derive_keywords(&j.action, &mode);
        Self {
            modifiers: j.modifiers,
            key: j.key,
            action: j.action,
            mode,
            origin: j.origin,
            keywords,
        }
    }

    /// Snapshot used as the fzf key field — the preview child parses this
    /// back out to render the binding's preview.
    fn to_payload(&self) -> KeybindPreviewPayload {
        KeybindPreviewPayload {
            modifiers: self.modifiers.clone(),
            key: self.key.clone(),
            action: self.action.clone(),
            mode: self.mode.clone(),
            origin: self.origin.clone(),
        }
    }

    /// Convenience wrapper used by [`KeybindPreviewPayload::binding`].
    pub(super) fn binding(&self) -> String {
        self.to_payload().binding()
    }

    /// Render the chord with bold key tokens and dimmed, unbolded plus separators
    /// so that combination `+` signs are easily distinguished from literal `+` keys.
    fn formatted_binding(&self) -> String {
        let separator = format!(" {} ", format_with_color("+", colors::OVERLAY0));
        let mut tokens: Vec<String> = self
            .modifiers
            .split('+')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(format_bold)
            .collect();

        if !self.key.is_empty() {
            tokens.push(format_bold(&self.key));
        }

        tokens.join(&separator)
    }

    /// Forwarded so the action submenu can keep using `row.sequence_steps()`
    /// without converting to a payload first.
    pub(super) fn sequence_steps(&self) -> Option<Vec<String>> {
        self.to_payload().sequence_steps()
    }

    /// Forwarded so the action submenu can keep using `row.origin_label()`.
    pub(super) fn origin_label(&self) -> &'static str {
        self.to_payload().origin_label()
    }
}

/// First token of an action string: the named action (`inc_master_count 1` →
/// `inc_master_count`). The whole string is returned when it has no args.
pub(super) fn action_name(action: &str) -> &str {
    action.split_whitespace().next().unwrap_or(action)
}

impl FzfSelectable for KeybindRow {
    fn fzf_display_text(&self) -> String {
        // `format_icon` emits its own ANSI reset. Key tokens are bolded with
        // subtle, unbolded plus separators, followed by an arrow separator
        // and the WM action name.
        format!(
            "{} {}  {}  {}",
            format_icon(NerdFont::Keyboard),
            self.formatted_binding(),
            format_with_color("→", colors::OVERLAY0),
            self.action,
        )
    }

    fn fzf_search_keywords(&self) -> &[&str] {
        &self.keywords
    }

    fn fzf_key(&self) -> String {
        // The fzf field is opaque to users but must be unique per row and
        // round-trippable into a [`KeybindPreviewPayload`]. JSON does both.
        serde_json::to_string(&self.to_payload()).unwrap_or_default()
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        // Lazy preview: fzf runs this command on highlight, passing the
        // payload as `$1`. `ins preview --id keyhelp` rebuilds the sectioned
        // preview (mode/origin/availability + action docs) on demand.
        crate::menu_utils::FzfPreview::Command(preview_command(PreviewId::Keyhelp))
    }
}
