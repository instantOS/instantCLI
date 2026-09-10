//! Typestate builder pattern for FZF dialogs.
//!
//! Each dialog kind (selection, input, password, confirmation, message,
//! checklist) is represented by its own concrete builder struct. Shared
//! configuration lives on [`FzfBuilder`] (the entry point); calling a
//! transition method (`input()`, `password()`, `confirm()`, `message()`,
//! `checklist()`) consumes the builder and yields a specialized builder that
//! exposes only the methods relevant to that dialog kind. This makes mistakes
//! like `.message(...).confirm_dialog()` impossible to express.
//!
//! Selection follows the same staged design. First choose exactly one source
//! with `items`, `stream`, or `command`; then compose applicable options
//! such as initial rows and keybinds; finally choose `select`, `select_many`,
//! or `select_one`. Adding an option therefore does not require another
//! cross-product terminal method.

mod checklist;
mod dialogs;
mod padded;
mod selection;
mod shared;

use anyhow::Result;

use crate::ui::catppuccin::format_icon_colored;
use crate::ui::nerd_font::NerdFont;

use super::types::*;

/// Configuration shared across every dialog kind. Carried forward through
/// transitions; specialized builders read from this for prompt, header,
/// additional args, etc.
#[derive(Debug, Clone)]
pub(crate) struct SharedConfig {
    pub prompt: Option<String>,
    pub header: Option<Header>,
    pub default_args: Vec<String>,
    pub user_args: Vec<String>,
    pub initial_cursor: Option<InitialCursor>,
    pub initial_query: Option<String>,
    pub responsive_layout: bool,
}

impl SharedConfig {
    fn new() -> Self {
        Self {
            prompt: None,
            header: None,
            default_args: default_args(),
            user_args: Vec::new(),
            initial_cursor: None,
            initial_query: None,
            responsive_layout: false,
        }
    }

    pub(crate) fn args(&self) -> impl Iterator<Item = &String> {
        self.default_args.iter().chain(self.user_args.iter())
    }

    fn with_dialog_args(mut self, defaults: Vec<String>) -> Self {
        self.default_args = defaults;
        self
    }
}

/// Entry-point builder. Carries shared configuration and exposes:
/// - shared setters (`prompt`, `header`, `args`, `initial_index`, `query`,
///   `responsive_layout`)
/// - selection-source transitions (`items`, `stream`, `command`)
/// - transitions to specialized builders (`input`, `password`, `confirm`,
///   `message`, `checklist`)
#[derive(Debug, Clone)]
pub struct FzfBuilder {
    pub(crate) shared: SharedConfig,
}

/// Selection backed by a complete in-memory collection.
pub struct ItemSelection<T, A = ()> {
    pub(crate) builder: FzfBuilder,
    pub(crate) items: Vec<T>,
    pub(crate) keybinds: Vec<MenuKeybind<A>>,
    pub(crate) presentation: ItemPresentation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ItemPresentation {
    Compact,
    Padded,
}

/// Selection backed by typed items arriving over a channel.
pub struct StreamSelection<'a, T, A = ()> {
    pub(crate) builder: FzfBuilder,
    pub(crate) initial_items: Vec<T>,
    pub(crate) late_items: crossbeam_channel::Receiver<T>,
    pub(crate) keybinds: Vec<MenuKeybind<A>>,
    pub(crate) on_ready: Option<Box<dyn FnOnce() -> Result<()> + 'a>>,
}

/// Selection backed by encoded rows emitted by a child process.
pub struct CommandSelection<T, A = ()> {
    pub(crate) builder: FzfBuilder,
    pub(crate) command: StreamingCommand,
    pub(crate) initial_rows: String,
    pub(crate) keybinds: Vec<MenuKeybind<A>>,
    pub(crate) payload: std::marker::PhantomData<T>,
}

#[derive(Debug, Clone)]
pub struct InputBuilder {
    pub(crate) shared: SharedConfig,
    pub(crate) ghost_text: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PasswordBuilder {
    pub(crate) shared: SharedConfig,
    pub(crate) confirm: bool,
}

#[derive(Debug, Clone)]
pub struct ConfirmBuilder {
    pub(crate) shared: SharedConfig,
    pub(crate) yes_text: String,
    pub(crate) no_text: String,
    pub(crate) title: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MessageBuilder {
    pub(crate) shared: SharedConfig,
    pub(crate) ok_text: String,
    pub(crate) title: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ChecklistBuilder {
    pub(crate) shared: SharedConfig,
    pub(crate) confirm_text: String,
    pub(crate) allow_empty: bool,
    pub(crate) actions: Vec<ChecklistAction>,
}

#[derive(Clone)]
pub(crate) struct ConfirmOption {
    pub(crate) label: String,
    pub(crate) color: &'static str,
    pub(crate) icon: NerdFont,
    pub(crate) result: ConfirmResult,
}

#[derive(Clone)]
pub(crate) struct ChecklistEntry {
    pub(crate) display: String,
    pub(crate) key: String,
    pub(crate) preview: FzfPreview,
}

impl ChecklistEntry {
    pub(crate) fn new(display: String, key: String, preview: FzfPreview) -> Self {
        Self {
            display,
            key,
            preview,
        }
    }
}

impl FzfSelectable for ChecklistEntry {
    fn fzf_display_text(&self) -> String {
        self.display.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview.clone()
    }

    fn fzf_key(&self) -> String {
        self.key.clone()
    }
}

impl ConfirmOption {
    pub(crate) fn new(
        label: String,
        color: &'static str,
        icon: NerdFont,
        result: ConfirmResult,
    ) -> Self {
        Self {
            label,
            color,
            icon,
            result,
        }
    }
}

impl FzfSelectable for ConfirmOption {
    fn fzf_display_text(&self) -> String {
        let badge = format_icon_colored(self.icon, self.color);
        format!("{badge}{}", self.label)
    }

    fn fzf_key(&self) -> String {
        self.label.clone()
    }
}

// ---------------------------------------------------------------------------
// Default fzf args per dialog kind
// ---------------------------------------------------------------------------

pub(crate) fn base_args(margin: &str) -> Vec<String> {
    let mut args = vec![
        "--margin".to_string(),
        margin.to_string(),
        "--min-height".to_string(),
        "10".to_string(),
    ];
    args.extend(super::theme::theme_args());
    args
}

pub(crate) fn default_args() -> Vec<String> {
    base_args("10%,2%")
}

pub(crate) fn input_args() -> Vec<String> {
    base_args("20%,2%")
}

pub(crate) fn confirm_args() -> Vec<String> {
    let mut args = base_args("20%,2%");
    args.push("--info=hidden".to_string());
    args.push("--color=header:-1".to_string());
    args.push("--no-input".to_string());
    args
}

pub(crate) fn password_args() -> Vec<String> {
    vec![]
}

pub(crate) fn checklist_args() -> Vec<String> {
    let mut args = base_args("10%,2%");
    args.push("--height=95%".to_string());
    args
}

// ---------------------------------------------------------------------------
// FzfBuilder (entry / Selection state)
// ---------------------------------------------------------------------------

impl Default for FzfBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl FzfBuilder {
    pub fn new() -> Self {
        Self {
            shared: SharedConfig::new(),
        }
    }

    pub fn prompt<S: Into<String>>(mut self, prompt: S) -> Self {
        self.shared.prompt = Some(prompt.into());
        self
    }

    pub fn header<H: Into<Header>>(mut self, header: H) -> Self {
        self.shared.header = Some(header.into());
        self
    }

    /// Set a known initial row. Use [`Self::cursor`] when restoring an
    /// optional cursor position.
    pub fn initial_index(self, index: usize) -> Self {
        self.cursor(Some(index))
    }

    /// Restore an optional initial row. Use [`Self::initial_index`] when the
    /// position is known.
    pub fn cursor(mut self, index: Option<usize>) -> Self {
        self.shared.initial_cursor = index.map(InitialCursor::Index);
        self
    }

    pub fn query<S: Into<String>>(mut self, query: S) -> Self {
        self.shared.initial_query = Some(query.into());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.shared
            .user_args
            .extend(args.into_iter().map(Into::into));
        self
    }

    pub fn responsive_layout(mut self) -> Self {
        self.shared.responsive_layout = true;
        self
    }

    // ---- transitions ----

    pub fn input(self) -> InputBuilder {
        let shared = self.shared.with_dialog_args(input_args());
        InputBuilder {
            shared,
            ghost_text: None,
        }
    }

    pub fn password(self) -> PasswordBuilder {
        let shared = self.shared.with_dialog_args(password_args());
        PasswordBuilder {
            shared,
            confirm: false,
        }
    }

    pub fn confirm<S: Into<String>>(self, message: S) -> ConfirmBuilder {
        let mut shared = self.shared.with_dialog_args(confirm_args());
        let message_str = message.into();
        if let Some(existing) = shared.header.take() {
            match existing {
                Header::Fancy(text) => {
                    shared.header = Some(Header::Fancy(format!("{text}\n\n{message_str}")));
                }
                Header::Default(text) => {
                    shared.header = Some(Header::Default(format!("{text}\n\n{message_str}")));
                }
                Header::Manual(text) => {
                    shared.header = Some(Header::Manual(format!("{text}\n\n{message_str}")));
                }
            }
        } else {
            shared.header = Some(Header::Default(message_str));
        }
        ConfirmBuilder {
            shared,
            yes_text: "Yes".to_string(),
            no_text: "No".to_string(),
            title: None,
        }
    }

    pub fn message<S: Into<String>>(self, message: S) -> MessageBuilder {
        let mut shared = self.shared.with_dialog_args(confirm_args());
        shared.header = Some(Header::Default(message.into()));
        MessageBuilder {
            shared,
            ok_text: "OK".to_string(),
            title: None,
        }
    }

    pub fn checklist<S: Into<String>>(self, confirm_text: S) -> ChecklistBuilder {
        let shared = self.shared.with_dialog_args(checklist_args());
        ChecklistBuilder {
            shared,
            confirm_text: confirm_text.into(),
            allow_empty: true,
            actions: Vec::new(),
        }
    }

    // ---- selection sources ----

    /// Use a complete in-memory collection as the selection source.
    ///
    /// Applicable options are configured on the returned value, followed by
    /// `select`, `select_many`, `select_one`, or `select_menu`.
    pub fn items<T>(self, items: Vec<T>) -> ItemSelection<T> {
        ItemSelection {
            builder: self,
            items,
            keybinds: Vec::new(),
            presentation: ItemPresentation::Compact,
        }
    }

    /// Use a complete in-memory collection rendered as spacious multiline rows.
    pub fn padded_items<T>(self, items: Vec<T>) -> ItemSelection<T> {
        ItemSelection {
            builder: self,
            items,
            keybinds: Vec::new(),
            presentation: ItemPresentation::Padded,
        }
    }

    /// Use a channel of typed items as the selection source.
    ///
    /// The menu opens immediately. Use `StreamSelection::initial_items` when
    /// some items should be visible before the first channel value arrives.
    pub fn stream<T>(
        self,
        late_items: crossbeam_channel::Receiver<T>,
    ) -> StreamSelection<'static, T> {
        StreamSelection {
            builder: self,
            initial_items: Vec::new(),
            late_items,
            keybinds: Vec::new(),
            on_ready: None,
        }
    }

    /// Use encoded menu rows emitted by a child process as the source.
    ///
    /// `T` is the payload decoded from each submitted row. Already encoded
    /// rows can be prepended with `CommandSelection::initial_rows`.
    pub fn command<T, C>(self, command: C) -> CommandSelection<T>
    where
        C: Into<StreamingCommand>,
    {
        CommandSelection {
            builder: self,
            command: command.into(),
            initial_rows: String::new(),
            keybinds: Vec::new(),
            payload: std::marker::PhantomData,
        }
    }
}

// ---------------------------------------------------------------------------
// InputBuilder
// ---------------------------------------------------------------------------

impl InputBuilder {
    pub fn ghost<S: Into<String>>(mut self, text: S) -> Self {
        self.ghost_text = Some(text.into());
        self
    }
}

// ---------------------------------------------------------------------------
// PasswordBuilder
// ---------------------------------------------------------------------------

impl PasswordBuilder {
    pub fn with_confirmation(mut self) -> Self {
        self.confirm = true;
        self
    }
}

// ---------------------------------------------------------------------------
// ConfirmBuilder
// ---------------------------------------------------------------------------

impl ConfirmBuilder {
    pub fn title<S: Into<String>>(mut self, title: S) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn yes_text<S: Into<String>>(mut self, text: S) -> Self {
        self.yes_text = text.into();
        self
    }

    pub fn no_text<S: Into<String>>(mut self, text: S) -> Self {
        self.no_text = text.into();
        self
    }
}

// ---------------------------------------------------------------------------
// MessageBuilder
// ---------------------------------------------------------------------------

impl MessageBuilder {
    pub fn title<S: Into<String>>(mut self, title: S) -> Self {
        self.title = Some(title.into());
        self
    }
}

// ---------------------------------------------------------------------------
// ChecklistBuilder
// ---------------------------------------------------------------------------

impl ChecklistBuilder {
    pub fn checklist_actions<I>(mut self, actions: I) -> Self
    where
        I: IntoIterator<Item = ChecklistAction>,
    {
        self.actions = actions.into_iter().collect();
        self
    }

    pub fn allow_empty_confirm(mut self, allow: bool) -> Self {
        self.allow_empty = allow;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_transitions_replace_defaults_but_preserve_user_args() {
        let input = FzfBuilder::new().args(["--color=fg:red"]).input();
        assert_eq!(input.shared.default_args, input_args());
        assert_eq!(input.shared.user_args, vec!["--color=fg:red"]);

        let checklist = FzfBuilder::new()
            .args(["--color=fg:green"])
            .checklist("Save");
        assert_eq!(checklist.shared.default_args, checklist_args());
        assert_eq!(checklist.shared.user_args, vec!["--color=fg:green"]);
    }

    #[test]
    fn selection_defaults_include_standard_style() {
        let builder = FzfBuilder::new();

        assert!(
            builder
                .shared
                .default_args
                .contains(&"--no-bold".to_string())
        );
        assert!(
            builder
                .shared
                .default_args
                .contains(&"--padding=1,2".to_string())
        );
        assert_eq!(
            builder.clone().items(Vec::<String>::new()).presentation,
            ItemPresentation::Compact
        );
        assert_eq!(
            builder.padded_items(Vec::<String>::new()).presentation,
            ItemPresentation::Padded
        );
    }
}
