//! Builder pattern for FZF dialogs

use anyhow::{self, Result};
use std::io::Write;
use std::process::{Command, Stdio};

use crate::common::shell::shell_quote;
use crate::ui::catppuccin::{colors, format_icon_colored, hex_to_ansi_bg, hex_to_ansi_fg};
use crate::ui::nerd_font::NerdFont;

use super::preview::PreviewUtils;
use super::types::*;
use super::utils::*;
use super::wrapper::{FzfWrapper, FzfWrapperParts, check_fzf_exit, configure_preview_and_input};

#[derive(Debug, Clone)]
pub struct FzfBuilder {
    multi_select: bool,
    prompt: Option<String>,
    header: Option<Header>,
    additional_args: Vec<String>,
    dialog_type: DialogType,
    initial_cursor: Option<InitialCursor>,
    initial_query: Option<String>,
    ghost_text: Option<String>,
    responsive_layout: bool,
    checklist_actions: Vec<ChecklistAction>,
}

#[derive(Debug, Clone)]
enum DialogType {
    Selection,
    Input,
    Password {
        confirm: bool,
    },
    Confirmation {
        yes_text: String,
        no_text: String,
    },
    Message {
        ok_text: String,
        title: Option<String>,
    },
    Checklist {
        confirm_text: String,
        allow_empty: bool,
    },
}

#[derive(Clone)]
struct ConfirmOption {
    label: String,
    color: &'static str,
    icon: NerdFont,
    result: ConfirmResult,
}

#[derive(Clone)]
struct ChecklistEntry {
    display: String,
    key: String,
    preview: FzfPreview,
}

impl ChecklistEntry {
    fn new(display: String, key: String, preview: FzfPreview) -> Self {
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
    fn new(label: String, color: &'static str, icon: NerdFont, result: ConfirmResult) -> Self {
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
impl FzfBuilder {
    pub fn new() -> Self {
        Self {
            multi_select: false,
            prompt: None,
            header: None,
            additional_args: Self::default_args(),
            dialog_type: DialogType::Selection,
            initial_cursor: None,
            initial_query: None,
            ghost_text: None,
            responsive_layout: false,
            checklist_actions: Vec::new(),
        }
    }

    pub(crate) fn into_wrapper_parts(self) -> FzfWrapperParts {
        FzfWrapperParts {
            multi_select: self.multi_select,
            prompt: self.prompt,
            header: self.header,
            additional_args: self.additional_args,
            initial_cursor: self.initial_cursor,
            responsive_layout: self.responsive_layout,
        }
    }

    pub fn multi_select(mut self, multi: bool) -> Self {
        self.multi_select = multi;
        self
    }

    pub fn prompt<S: Into<String>>(mut self, prompt: S) -> Self {
        self.prompt = Some(prompt.into());
        self
    }

    pub fn header<H: Into<Header>>(mut self, header: H) -> Self {
        self.header = Some(header.into());
        self
    }

    pub fn initial_index(mut self, index: usize) -> Self {
        self.initial_cursor = Some(InitialCursor::Index(index));
        self
    }

    /// Set an initial query to prepopulate the input field
    pub fn query<S: Into<String>>(mut self, query: S) -> Self {
        self.initial_query = Some(query.into());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.additional_args
            .extend(args.into_iter().map(Into::into));
        self
    }

    /// Enable responsive layout: preview window position adapts to terminal dimensions.
    /// Uses bottom preview for narrow (< 60 cols) or square-ish (aspect ratio < 2:1) terminals.
    /// Uses right preview for wide terminals with aspect ratio >= 2:1.
    pub fn responsive_layout(mut self) -> Self {
        self.responsive_layout = true;
        self
    }

    pub fn input(mut self) -> Self {
        self.dialog_type = DialogType::Input;
        self.additional_args = Self::input_args();
        self
    }

    /// Set ghost/placeholder text shown when input is empty
    pub fn ghost<S: Into<String>>(mut self, text: S) -> Self {
        self.ghost_text = Some(text.into());
        self
    }

    pub fn password(mut self) -> Self {
        self.dialog_type = DialogType::Password { confirm: false };
        self.additional_args = Self::password_args();
        self
    }

    pub fn with_confirmation(mut self) -> Self {
        if let DialogType::Password { ref mut confirm } = self.dialog_type {
            *confirm = true;
        }
        self
    }

    pub fn confirm<S: Into<String>>(mut self, message: S) -> Self {
        self.dialog_type = DialogType::Confirmation {
            yes_text: "Yes".to_string(),
            no_text: "No".to_string(),
        };
        self.header = Some(Header::Default(message.into()));
        self.additional_args = Self::confirm_args();
        self
    }

    pub fn yes_text<S: Into<String>>(mut self, text: S) -> Self {
        if let DialogType::Confirmation { yes_text, .. } = &mut self.dialog_type {
            *yes_text = text.into();
        }
        self
    }

    pub fn no_text<S: Into<String>>(mut self, text: S) -> Self {
        if let DialogType::Confirmation { no_text, .. } = &mut self.dialog_type {
            *no_text = text.into();
        }
        self
    }

    pub fn message<S: Into<String>>(mut self, message: S) -> Self {
        self.dialog_type = DialogType::Message {
            ok_text: "OK".to_string(),
            title: None,
        };
        self.header = Some(Header::Default(message.into()));
        self.additional_args = Self::confirm_args();
        self
    }

    pub fn title<S: Into<String>>(mut self, title: S) -> Self {
        if let DialogType::Message { title: target, .. } = &mut self.dialog_type {
            *target = Some(title.into());
        }
        self
    }

    /// Configure the builder as a checklist dialog.
    /// Users can toggle items with Enter, then confirm by selecting the confirm option.
    pub fn checklist<S: Into<String>>(mut self, confirm_text: S) -> Self {
        self.dialog_type = DialogType::Checklist {
            confirm_text: confirm_text.into(),
            allow_empty: true,
        };
        self.additional_args = Self::checklist_args();
        self
    }

    /// Add action buttons (non-checkbox items) to a checklist dialog.
    pub fn checklist_actions<I>(mut self, actions: I) -> Self
    where
        I: IntoIterator<Item = ChecklistAction>,
    {
        self.checklist_actions = actions.into_iter().collect();
        self
    }

    /// Set whether the checklist allows confirming with no items checked.
    pub fn allow_empty_confirm(mut self, allow: bool) -> Self {
        if let DialogType::Checklist { allow_empty, .. } = &mut self.dialog_type {
            *allow_empty = allow;
        }
        self
    }

    /// Execute checklist dialog and return checked items or selected action.
    pub fn checklist_dialog<T: FzfSelectable + Clone>(
        self,
        items: Vec<T>,
    ) -> Result<ChecklistResult<T>> {
        if !matches!(self.dialog_type, DialogType::Checklist { .. }) {
            return Err(anyhow::anyhow!("Builder not configured for checklist"));
        }
        self.execute_checklist(items)
    }

    pub fn select<T: FzfSelectable + Clone>(self, items: Vec<T>) -> Result<FzfResult<T>> {
        FzfWrapper::from_builder(self).select(items)
    }

    /// Select from a list that may contain `MenuItem::Separator` entries.
    ///
    /// Separators are rendered as dimmed lines and cursor navigation skips them.
    /// Returns `FzfResult<T>` (unwrapped from `MenuItem`), never a separator.
    /// If the user somehow selects a separator, the menu re-launches with the
    /// cursor moved to the nearest selectable item.
    pub fn select_menu<T: FzfSelectable + Clone>(
        self,
        items: Vec<super::types::MenuItem<T>>,
    ) -> Result<FzfResult<T>> {
        use super::types::MenuItem;

        let mut wrapper = FzfWrapper::from_builder(self);

        loop {
            match wrapper.select(items.clone())? {
                FzfResult::Selected(MenuItem::Entry(item)) => {
                    return Ok(FzfResult::Selected(item));
                }
                FzfResult::Selected(MenuItem::Separator(_)) => {
                    wrapper.initial_cursor = None;
                    continue;
                }
                FzfResult::MultiSelected(selected) => {
                    let entries: Vec<T> = selected
                        .into_iter()
                        .filter_map(|mi| match mi {
                            MenuItem::Entry(item) => Some(item),
                            MenuItem::Separator(_) => None,
                        })
                        .collect();
                    return Ok(FzfResult::MultiSelected(entries));
                }
                FzfResult::Cancelled => return Ok(FzfResult::Cancelled),
                FzfResult::Error(e) => return Ok(FzfResult::Error(e)),
            }
        }
    }

    /// Select with vertical padding around each item.
    /// Uses NUL-separated multi-line items so the entire padded area is highlighted.
    /// Uses FZF's {n} index for reliable item matching instead of parsing display text.
    pub fn select_padded<T: FzfSelectable + Clone>(self, items: Vec<T>) -> Result<FzfResult<T>> {
        if items.is_empty() {
            return Ok(FzfResult::Cancelled);
        }

        // Check if any item has keywords to determine if we need hidden keyword support
        let has_keywords = items
            .iter()
            .any(|item| !item.fzf_search_keywords().is_empty());

        let input_text = Self::prepare_padded_input(&items);
        let has_preview = items
            .iter()
            .any(|item| !matches!(item.fzf_preview(), FzfPreview::None));
        let preview_dir = if has_preview {
            Some(Self::prepare_padded_previews(&items)?)
        } else {
            None
        };

        let mut cmd = self.configure_padded_cmd(preview_dir.as_deref(), has_keywords);

        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let pid = child.id();
        let _ = crate::menu::server::register_menu_process(pid);

        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(input_text.as_bytes())?;
        }

        let output = child.wait_with_output();
        crate::menu::server::unregister_menu_process(pid);

        // Clean up temp preview directory
        if let Some(dir) = preview_dir.as_ref() {
            let _ = std::fs::remove_dir_all(dir);
        }

        match output {
            Ok(result) => {
                if let Some(cancelled) = check_fzf_exit(&result) {
                    return Ok(cancelled);
                }

                if !result.status.success() {
                    return Ok(FzfResult::Cancelled);
                }

                let stdout = String::from_utf8_lossy(&result.stdout);
                let index_str = stdout.trim();

                if index_str.is_empty() {
                    return Ok(FzfResult::Cancelled);
                }

                // Parse the index from FZF's output
                if let Ok(index) = index_str.parse::<usize>()
                    && let Some(item) = items.get(index)
                {
                    return Ok(FzfResult::Selected(item.clone()));
                }

                Ok(FzfResult::Cancelled)
            }
            Err(e) => Ok(FzfResult::Error(format!("fzf execution failed: {e}"))),
        }
    }

    fn prepare_padded_input<T: FzfSelectable>(items: &[T]) -> String {
        let mut input_lines = Vec::new();
        // Padding to push keywords off-screen (searchable but not visible)
        const HIDDEN_PADDING: &str = "                                                                                                    ";

        for item in items {
            let display = item.fzf_display_text();
            // Extract background color from display text to create matching padding
            // The icon badge format is: {bg}{fg}  {icon}  {reset} ...
            // We want padding lines to have the same colored block at the start,
            // with a subtle shadow on the bottom padding using a lower block character
            let (top_padding, bottom_with_shadow) = extract_icon_padding(&display);
            // Get search keywords for this item
            let keywords = item.fzf_search_keywords().join(" ");

            // Only apply padding/delimiter trick when this item has keywords
            let middle_line = if keywords.is_empty() {
                format!("  {display}")
            } else {
                format!("  {display}{HIDDEN_PADDING}\x1f{keywords}")
            };

            let padded_item = format!("{top_padding}\n{middle_line}\n{bottom_with_shadow}");
            input_lines.push(padded_item);
        }

        input_lines.join("\0")
    }

    fn prepare_padded_previews<T: FzfSelectable>(items: &[T]) -> Result<std::path::PathBuf> {
        let preview_dir = std::env::temp_dir().join(format!("fzf_preview_{}", std::process::id()));
        std::fs::create_dir_all(&preview_dir)?;

        for (idx, item) in items.iter().enumerate() {
            match item.fzf_preview() {
                FzfPreview::Text(preview) => {
                    let preview_path = preview_dir.join(format!("{}.txt", idx));
                    if let Ok(mut file) = std::fs::File::create(&preview_path) {
                        let _ = file.write_all(preview.as_bytes());
                    }
                }
                FzfPreview::Command(cmd) => {
                    // Write a shell script that FZF can execute
                    let preview_path = preview_dir.join(format!("{}.sh", idx));
                    if let Ok(mut file) = std::fs::File::create(&preview_path) {
                        let key = shell_quote(&item.fzf_key());
                        let script = format!("set -- {key}\n{cmd}");
                        let _ = file.write_all(script.as_bytes());
                    }
                }
                FzfPreview::None => {}
            }
        }
        Ok(preview_dir)
    }

    fn configure_padded_cmd(
        &self,
        preview_dir: Option<&std::path::Path>,
        has_keywords: bool,
    ) -> Command {
        let mut cmd = Command::new("fzf");
        cmd.env_remove("FZF_DEFAULT_OPTS");

        // Core options for multi-line items
        cmd.arg("--read0"); // NUL-separated input
        cmd.arg("--ansi"); // ANSI color support
        cmd.arg("--highlight-line"); // Highlight entire multi-line item
        cmd.arg("--layout=reverse");
        cmd.arg("--tiebreak=index");
        cmd.arg("--info=inline-right");

        // Only use delimiter and no-hscroll when at least one item has hidden keywords
        if has_keywords {
            cmd.arg("--delimiter=\x1f").arg("--no-hscroll");
        }

        // Use --bind to print the index on accept instead of the selection text
        // {n} is the 0-based index of the selected item
        cmd.arg("--bind").arg("enter:become(echo {n})");

        // Preview command using {n} for the 0-based item index
        // Check for .sh script first (Command preview), then .txt (Text preview)
        if let Some(dir) = preview_dir {
            let preview_cmd = format!(
                "if [ -f {dir}/{{n}}.sh ]; then bash {dir}/{{n}}.sh; elif [ -f {dir}/{{n}}.txt ]; then cat {dir}/{{n}}.txt; fi",
                dir = dir.display()
            );
            cmd.arg("--preview").arg(&preview_cmd);
        }

        // Apply prompt and header
        if let Some(prompt) = &self.prompt {
            cmd.arg("--prompt").arg(format!("{prompt} > "));
        }
        if let Some(header) = &self.header {
            cmd.arg("--header").arg(header.to_fzf_string());
        }

        // Apply initial cursor position
        if let Some(InitialCursor::Index(index)) = self.initial_cursor {
            cmd.arg("--bind").arg(format!("load:pos({})", index + 1));
        }

        // Apply all additional args (styling, colors, etc.)
        for arg in &self.additional_args {
            cmd.arg(arg);
        }

        // Apply responsive layout settings LAST to override defaults
        // Preview position and margins adapt to terminal dimensions
        if self.responsive_layout {
            let layout = super::utils::get_responsive_layout();
            cmd.arg(layout.preview_window);
            cmd.arg("--margin").arg(layout.margin);
        }

        cmd
    }

    pub fn select_streaming(self, command: &str) -> Result<FzfResult<String>> {
        FzfWrapper::from_builder(self).select_streaming(command)
    }

    pub fn input_dialog(self) -> Result<String> {
        if !matches!(self.dialog_type, DialogType::Input) {
            return Err(anyhow::anyhow!("Builder not configured for input"));
        }
        self.execute_input()
    }

    pub fn password_dialog(self) -> Result<FzfResult<String>> {
        let confirm = if let DialogType::Password { confirm } = self.dialog_type {
            confirm
        } else {
            return Err(anyhow::anyhow!("Builder not configured for password"));
        };
        self.execute_password(confirm)
    }

    pub fn confirm_dialog(self) -> Result<ConfirmResult> {
        if !matches!(self.dialog_type, DialogType::Confirmation { .. }) {
            return Err(anyhow::anyhow!("Builder not configured for confirmation"));
        }
        self.execute_confirm()
    }

    pub fn message_dialog(self) -> Result<()> {
        if !matches!(self.dialog_type, DialogType::Message { .. }) {
            return Err(anyhow::anyhow!("Builder not configured for message"));
        }
        self.execute_message()
    }

    pub fn show_confirmation(self) -> Result<ConfirmResult> {
        self.confirm_dialog()
    }

    pub fn show_message(self) -> Result<()> {
        self.message_dialog()
    }

    /// Returns FzfResult with explicit Cancelled variant, unlike input_dialog() which returns empty string.
    /// Use this when you need to distinguish between user cancellation and empty input.
    pub fn input_result(self) -> Result<FzfResult<String>> {
        if !matches!(self.dialog_type, DialogType::Input) {
            return Err(anyhow::anyhow!("Builder not configured for input"));
        }
        self.execute_input_result()
    }

    fn execute_input(self) -> Result<String> {
        match self.execute_input_result()? {
            FzfResult::Selected(s) => Ok(s),
            FzfResult::Cancelled => Ok(String::new()),
            FzfResult::Error(e) => Err(anyhow::anyhow!(e)),
            _ => Ok(String::new()),
        }
    }

    fn execute_input_result(self) -> Result<FzfResult<String>> {
        let mut cmd = Command::new("fzf");
        cmd.env_remove("FZF_DEFAULT_OPTS");
        cmd.arg("--print-query").arg("--no-info");

        if let Some(prompt) = &self.prompt {
            cmd.arg("--prompt").arg(format!("{prompt} "));
        }

        if let Some(query) = &self.initial_query {
            cmd.arg("-q").arg(query);
        }

        if let Some(ghost) = &self.ghost_text {
            cmd.arg("--ghost").arg(ghost);
        }

        for arg in &self.additional_args {
            cmd.arg(arg);
        }

        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let pid = child.id();
        let _ = crate::menu::server::register_menu_process(pid);

        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(b"")?;
        }

        let output = child.wait_with_output()?;
        crate::menu::server::unregister_menu_process(pid);

        if let Some(cancelled) = check_fzf_exit(&output) {
            return Ok(cancelled);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.trim_end().split('\n').collect();

        if let Some(query) = lines.first() {
            Ok(FzfResult::Selected(query.trim().to_string()))
        } else {
            Ok(FzfResult::Selected(String::new()))
        }
    }

    fn execute_password(self, confirm: bool) -> Result<FzfResult<String>> {
        loop {
            let header_str = self.header.as_ref().map(|h| h.to_fzf_string());
            let pass1 = self.run_password_prompt(self.prompt.as_deref(), header_str.as_deref())?;

            if !confirm {
                return Ok(pass1);
            }

            let pass1_str = match pass1 {
                FzfResult::Selected(s) => s,
                _ => return Ok(pass1),
            };

            let pass2 = self.run_password_prompt(Some("Confirm password"), None)?;

            let pass2_str = match pass2 {
                FzfResult::Selected(s) => s,
                _ => return Ok(pass2),
            };

            if pass1_str == pass2_str {
                return Ok(FzfResult::Selected(pass1_str));
            }

            FzfWrapper::message("Passwords do not match. Please try again.")?;
        }
    }

    fn run_password_prompt(
        &self,
        prompt: Option<&str>,
        header: Option<&str>,
    ) -> Result<FzfResult<String>> {
        let mut cmd = Command::new("gum");
        cmd.arg("input").arg("--password");

        if let Some(p) = prompt {
            cmd.arg("--prompt").arg(format!("{p} "));
        }

        cmd.arg("--padding").arg("1 2");
        cmd.arg("--width").arg("60");

        if let Some(h) = header {
            cmd.arg("--placeholder").arg(h);
        } else {
            cmd.arg("--placeholder").arg("Enter your password");
        }

        let child = cmd
            .stdin(Stdio::inherit())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn();

        match child {
            Ok(child) => {
                let output = child.wait_with_output()?;

                if let Some(code) = output.status.code()
                    && (code == 130 || code == 143)
                {
                    return Ok(FzfResult::Cancelled);
                }

                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    Ok(FzfResult::Selected(stdout.trim().to_string()))
                } else {
                    self.fallback_password_input(prompt)
                }
            }
            Err(_) => self.fallback_password_input(prompt),
        }
    }

    fn fallback_password_input(&self, prompt: Option<&str>) -> Result<FzfResult<String>> {
        use std::io::Write as _;

        eprint!("{}: ", prompt.unwrap_or("Enter password"));
        let _ = std::io::stderr().flush();

        let mut password = String::new();
        let bytes = std::io::stdin().read_line(&mut password)?;

        if bytes == 0 {
            return Ok(FzfResult::Cancelled);
        }

        Ok(FzfResult::Selected(password.trim().to_string()))
    }

    fn execute_confirm(mut self) -> Result<ConfirmResult> {
        let (yes_text, no_text) = if let DialogType::Confirmation {
            ref yes_text,
            ref no_text,
        } = self.dialog_type
        {
            (yes_text.clone(), no_text.clone())
        } else {
            return Ok(ConfirmResult::Cancelled);
        };

        let header_text = Self::format_message_header(None, self.header.as_ref());
        if !header_text.is_empty() {
            self.header = Some(Header::Manual(header_text));
        }

        let options = vec![
            ConfirmOption::new(yes_text, colors::GREEN, NerdFont::Check, ConfirmResult::Yes),
            ConfirmOption::new(no_text, colors::RED, NerdFont::Cross, ConfirmResult::No),
        ];

        match self.select_padded(options)? {
            FzfResult::Selected(option) => Ok(option.result),
            FzfResult::Cancelled => Ok(ConfirmResult::Cancelled),
            _ => Ok(ConfirmResult::Cancelled),
        }
    }

    fn execute_message(self) -> Result<()> {
        let (ok_text, title) = if let DialogType::Message {
            ref ok_text,
            ref title,
        } = self.dialog_type
        {
            (ok_text.clone(), title.clone())
        } else {
            return Ok(());
        };

        let mut cmd = Command::new("fzf");
        cmd.env_remove("FZF_DEFAULT_OPTS");
        cmd.arg("--layout").arg("reverse");
        cmd.arg("--wrap");
        cmd.arg("--read0"); // NUL-separated for multi-line items
        cmd.arg("--ansi");
        cmd.arg("--highlight-line");

        // Build styled header with title and message
        let header_text = Self::format_message_header(title.as_deref(), self.header.as_ref());
        if !header_text.is_empty() {
            cmd.arg("--header").arg(header_text);
        }

        // Hide input field since message dialogs don't need text input
        cmd.arg("--no-input");

        for arg in &self.additional_args {
            cmd.arg(arg);
        }

        // Create styled OK button with catppuccin colors
        let ok_styled = Self::format_styled_button(&ok_text, colors::GREEN, NerdFont::Check);

        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let pid = child.id();
        let _ = crate::menu::server::register_menu_process(pid);

        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(ok_styled.as_bytes())?;
        }

        let output = child.wait_with_output()?;
        crate::menu::server::unregister_menu_process(pid);

        if check_fzf_exit::<()>(&output).is_some() {
            return Ok(());
        }

        Ok(())
    }

    /// Format a styled button with icon badge sampling from catppuccin colors
    fn format_styled_button(text: &str, color: &str, icon: NerdFont) -> String {
        let bg = hex_to_ansi_bg(color);
        let fg = hex_to_ansi_fg(colors::CRUST);
        let reset = "\x1b[49;39m";

        // Create the display line with icon badge
        let icon = char::from(icon);
        let display_line = format!("{bg}{fg}   {icon}   {reset}  {text}");

        // Create padding lines with shadow effect
        let (top_padding, bottom_with_shadow) = extract_icon_padding(&display_line);

        // Multi-line padded item
        format!("{top_padding}\n  {display_line}\n{bottom_with_shadow}")
    }

    /// Format a styled message header with title and separator (inspired by PreviewBuilder)
    fn format_message_header(title: Option<&str>, message: Option<&Header>) -> String {
        const RESET: &str = "\x1b[0m";
        const SEPARATOR: &str = "───────────────────────────────────";

        let mut lines = Vec::new();

        // Add styled title if present
        if let Some(t) = title {
            let mauve = hex_to_ansi_fg(colors::MAUVE);
            let bold = "\x1b[1m";
            lines.push(format!("{bold}{mauve}{t}{RESET}"));

            // Add separator below title
            let surface = hex_to_ansi_fg(colors::SURFACE1);
            lines.push(format!("{surface}{SEPARATOR}{RESET}"));
        }

        // Add message if present (with text wrapping)
        if let Some(header) = message {
            let text_color = hex_to_ansi_fg(colors::TEXT);
            let msg_text = header.to_fzf_string();
            // Use terminal width for wrapping, with padding for margins
            let wrap_width = get_terminal_dimensions()
                .map(|(cols, _)| (cols as usize).saturating_sub(10))
                .unwrap_or(60)
                .max(40); // Minimum 40 chars
            for wrapped_line in Self::wrap_text(&msg_text, wrap_width) {
                lines.push(format!("{text_color}{wrapped_line}{RESET}"));
            }
            // Add blank lines to separate from OK button
            lines.push(String::new());
            lines.push(String::new());
        }

        lines.join("\n")
    }

    /// Wrap text at word boundaries to fit within max_width characters.
    /// Preserves existing newlines in the input text.
    fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
        let mut output_lines = Vec::new();

        // Process each input line separately to preserve newlines
        for input_line in text.lines() {
            if input_line.is_empty() {
                output_lines.push(String::new());
                continue;
            }

            // Wrap this single line if needed
            let mut current_line = String::new();
            for word in input_line.split_whitespace() {
                if current_line.is_empty() {
                    current_line = word.to_string();
                } else if current_line.len() + 1 + word.len() <= max_width {
                    current_line.push(' ');
                    current_line.push_str(word);
                } else {
                    output_lines.push(current_line);
                    current_line = word.to_string();
                }
            }
            if !current_line.is_empty() {
                output_lines.push(current_line);
            }
        }

        if output_lines.is_empty() {
            output_lines.push(String::new());
        }

        output_lines
    }

    fn execute_checklist<T: FzfSelectable + Clone>(
        self,
        items: Vec<T>,
    ) -> Result<ChecklistResult<T>> {
        if items.is_empty() {
            return Ok(ChecklistResult::Cancelled);
        }

        let (confirm_text, allow_empty) = match &self.dialog_type {
            DialogType::Checklist {
                confirm_text,
                allow_empty,
            } => (confirm_text.clone(), *allow_empty),
            _ => return Err(anyhow::anyhow!("Not a checklist dialog")),
        };

        // Create checklist state with wrapped items
        let mut checklist_items: Vec<ChecklistItem<T>> =
            items.into_iter().map(ChecklistItem::new).collect();

        // Create confirm option
        let confirm_item = ChecklistConfirm::new(&confirm_text);

        let action_items = self.checklist_actions.clone();
        let mut action_map: std::collections::HashMap<String, ChecklistAction> =
            std::collections::HashMap::new();
        for action in &action_items {
            action_map.insert(action.fzf_key(), action.clone());
        }

        // Track cursor position across FZF restarts
        let mut cursor: Option<usize> = None;

        loop {
            // Build key-to-index map and input text
            let mut key_to_index: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            let mut entries = Vec::new();

            for (idx, item) in checklist_items.iter().enumerate() {
                let display = item.fzf_display_text();
                let key = item.fzf_key();
                let preview = item.fzf_preview();
                key_to_index.insert(key.clone(), idx);
                entries.push(ChecklistEntry::new(display, key, preview));
            }

            // Add action items (non-checkbox)
            for action in &action_items {
                entries.push(ChecklistEntry::new(
                    action.fzf_display_text(),
                    action.fzf_key(),
                    action.fzf_preview(),
                ));
            }

            // Add confirm option at the end
            entries.push(ChecklistEntry::new(
                confirm_item.fzf_display_text(),
                confirm_item.fzf_key(),
                confirm_item.fzf_preview(),
            ));

            // Execute FZF with current state and cursor position
            let result = self.run_checklist_fzf(&entries, &key_to_index, &action_map, cursor)?;

            match result {
                ChecklistSelection::Cancelled => return Ok(ChecklistResult::Cancelled),
                ChecklistSelection::EmptyQuery => {
                    // User pressed Enter with empty query - ask if they want to discard selections
                    // Check if there are any checked items
                    let has_checked = checklist_items.iter().any(|item| item.checked);

                    if has_checked {
                        match FzfWrapper::builder()
                            .confirm("Discard selections and exit?")
                            .yes_text("Discard")
                            .no_text("Keep")
                            .confirm_dialog()?
                        {
                            ConfirmResult::Yes => return Ok(ChecklistResult::Cancelled),
                            ConfirmResult::No => continue,
                            ConfirmResult::Cancelled => continue,
                        }
                    } else {
                        // No checked items, just continue the loop
                        continue;
                    }
                }
                ChecklistSelection::NotFound => {
                    // User typed a query that doesn't match any items
                    // Show message and continue the loop
                    FzfWrapper::message("No matching items found")?;
                    continue;
                }
                ChecklistSelection::Toggled(index) => {
                    // Toggle the item at index (if it's a valid item index)
                    if let Some(item) = checklist_items.get_mut(index) {
                        item.toggle();
                    }
                    // Update cursor to stay on this item for next iteration
                    cursor = Some(index);
                    // Loop continues - FZF will reopen with updated checkboxes
                }
                ChecklistSelection::Confirmed => {
                    // Collect indices of checked items first
                    let checked_indices: Vec<usize> = checklist_items
                        .iter()
                        .enumerate()
                        .filter(|(_, item)| item.checked)
                        .map(|(idx, _)| idx)
                        .collect();

                    if !allow_empty && checked_indices.is_empty() {
                        // Show error and loop again
                        FzfWrapper::message("Please select at least one item before confirming.")?;
                        continue;
                    }

                    // Now extract the checked items
                    let checked: Vec<T> = checklist_items
                        .into_iter()
                        .enumerate()
                        .filter(|(idx, _)| checked_indices.contains(idx))
                        .map(|(_, item)| item.item)
                        .collect();

                    return Ok(ChecklistResult::Confirmed(checked));
                }
                ChecklistSelection::Action(key) => {
                    if let Some(action) = action_map.get(&key) {
                        return Ok(ChecklistResult::Action(action.clone()));
                    }
                }
            }
        }
    }

    fn run_checklist_fzf(
        &self,
        entries: &[ChecklistEntry],
        key_to_index: &std::collections::HashMap<String, usize>,
        action_map: &std::collections::HashMap<String, ChecklistAction>,
        cursor: Option<usize>,
    ) -> Result<ChecklistSelection> {
        let mut cmd = Command::new("fzf");
        cmd.env_remove("FZF_DEFAULT_OPTS");
        cmd.arg("--ansi");
        cmd.arg("--tiebreak=index");
        cmd.arg("--layout=reverse");
        cmd.arg("--print-query"); // Always print the query, even if no match

        // Configure prompt
        if let Some(prompt) = &self.prompt {
            cmd.arg("--prompt").arg(format!("{prompt} > "));
        }

        // Configure header
        if let Some(header) = &self.header {
            cmd.arg("--header").arg(header.to_fzf_string());
        }

        // Apply additional args (contains toggle bindings from checklist_args)
        for arg in &self.additional_args {
            cmd.arg(arg);
        }

        // Apply cursor position to preserve position across FZF restarts
        if let Some(index) = cursor {
            cmd.arg("--bind").arg(format!("load:pos({})", index + 1));
        }

        // Apply responsive layout if enabled
        if self.responsive_layout {
            let layout = super::utils::get_responsive_layout();
            cmd.arg(layout.preview_window);
            cmd.arg("--margin").arg(layout.margin);
        }

        let preview_strategy = PreviewUtils::analyze_preview_strategy(entries)?;
        let display_data: Vec<(String, String, String, bool)> = entries
            .iter()
            .map(|entry| {
                (
                    entry.fzf_display_text(),
                    entry.fzf_key(),
                    entry.fzf_search_keywords().join(" "),
                    true,
                )
            })
            .collect();
        let input_text =
            configure_preview_and_input(&mut cmd, preview_strategy, &display_data, false);

        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let pid = child.id();
        let _ = crate::menu::server::register_menu_process(pid);

        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(input_text.as_bytes())?;
        }

        let output = child.wait_with_output()?;
        crate::menu::server::unregister_menu_process(pid);

        // Parse output
        self.parse_checklist_output(output, key_to_index, action_map)
    }

    fn parse_checklist_output(
        &self,
        result: std::process::Output,
        key_to_index: &std::collections::HashMap<String, usize>,
        action_map: &std::collections::HashMap<String, ChecklistAction>,
    ) -> Result<ChecklistSelection> {
        let exit_code = result.status.code();

        // Handle cancellation and fzf errors (except code 1 which means no match)
        if exit_code != Some(1) {
            if let Some(_) = check_fzf_exit::<()>(&result) {
                return Ok(ChecklistSelection::Cancelled);
            }
        } else if !result.status.success() {
            // code 1 with no match is handled below via empty selection
        }

        let stdout = String::from_utf8_lossy(&result.stdout);
        let lines: Vec<&str> = stdout.trim_end().split('\n').collect();

        // With --print-query, output is:
        // Line 1: query (what user typed)
        // Line 2: selected item (if any)

        let query = lines.first().map(|s| s.trim()).unwrap_or("");
        let selected = lines.get(1).map(|s| s.trim()).unwrap_or("");

        // No selection - check if query is empty or not
        if selected.is_empty() {
            if query.is_empty() {
                // User pressed Enter with empty query
                return Ok(ChecklistSelection::EmptyQuery);
            } else {
                // User typed a query that doesn't match anything
                return Ok(ChecklistSelection::NotFound);
            }
        }

        // Extract the key from selected line (format: display\x1fkeywords\x1fkey)
        if let Some(key) = selected.split('\x1f').nth(2) {
            // Check if it's the confirm action
            if key == ChecklistConfirm::confirm_key() {
                return Ok(ChecklistSelection::Confirmed);
            }

            if action_map.contains_key(key) {
                return Ok(ChecklistSelection::Action(key.to_string()));
            }

            // Look up the index for this key
            if let Some(&index) = key_to_index.get(key) {
                return Ok(ChecklistSelection::Toggled(index));
            }
        }

        // Selection doesn't match any of our items - treat as not found
        Ok(ChecklistSelection::NotFound)
    }

    /// Base args with margin and theme, used by other *_args functions.
    fn base_args(margin: &str) -> Vec<String> {
        let mut args = vec![
            "--margin".to_string(),
            margin.to_string(),
            "--min-height".to_string(),
            "10".to_string(),
        ];
        args.extend(
            super::theme::theme_args()
                .into_iter()
                .map(|s| s.to_string()),
        );
        args
    }

    fn default_args() -> Vec<String> {
        Self::base_args("10%,2%")
    }

    fn input_args() -> Vec<String> {
        Self::base_args("20%,2%")
    }

    fn confirm_args() -> Vec<String> {
        let mut args = Self::base_args("20%,2%");
        args.push("--info=hidden".to_string());
        args.push("--color=header:-1".to_string());
        args.push("--no-input".to_string());
        args
    }

    fn password_args() -> Vec<String> {
        vec![]
    }

    fn checklist_args() -> Vec<String> {
        let mut args = Self::base_args("10%,2%");
        args.push("--height=95%".to_string()); // Give more space for checklist
        // Don't use --multi - we use single-select with loop/reload pattern
        // No special key bindings needed - use default fzf behavior
        args
    }
}
