//! Builder pattern for FZF dialogs

use anyhow::{self, Result};
use std::io::Write;
use std::process::{Command, Stdio};

use super::types::*;
use super::utils::*;
use super::wrapper::FzfWrapper;

#[derive(Debug, Clone)]
pub struct FzfBuilder {
    multi_select: bool,
    prompt: Option<String>,
    header: Option<Header>,
    additional_args: Vec<String>,
    dialog_type: DialogType,
    initial_cursor: Option<InitialCursor>,
    initial_query: Option<String>,
    responsive_layout: bool,
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
            responsive_layout: false,
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
    /// Users can toggle items with Tab or Enter, then confirm by selecting the confirm option.
    pub fn checklist<S: Into<String>>(mut self, confirm_text: S) -> Self {
        self.dialog_type = DialogType::Checklist {
            confirm_text: confirm_text.into(),
            allow_empty: true,
        };
        self.additional_args = Self::checklist_args();
        self
    }

    /// Set whether the checklist allows confirming with no items checked.
    pub fn allow_empty_confirm(mut self, allow: bool) -> Self {
        if let DialogType::Checklist { allow_empty, .. } = &mut self.dialog_type {
            *allow_empty = allow;
        }
        self
    }

    /// Execute checklist dialog and return checked items.
    /// Returns FzfResult::MultiSelected(Vec<T>) with checked items.
    pub fn checklist_dialog<T: FzfSelectable + Clone>(self, items: Vec<T>) -> Result<FzfResult<T>> {
        if !matches!(self.dialog_type, DialogType::Checklist { .. }) {
            return Err(anyhow::anyhow!("Builder not configured for checklist"));
        }
        self.execute_checklist(items)
    }

    pub fn select<T: FzfSelectable + Clone>(self, items: Vec<T>) -> Result<FzfResult<T>> {
        let wrapper = FzfWrapper::new(
            self.multi_select,
            self.prompt,
            self.header,
            self.additional_args,
            self.initial_cursor,
            self.responsive_layout,
        );
        wrapper.select(items)
    }

    /// Select with vertical padding around each item.
    /// Uses NUL-separated multi-line items so the entire padded area is highlighted.
    /// Uses FZF's {n} index for reliable item matching instead of parsing display text.
    pub fn select_padded<T: FzfSelectable + Clone>(self, items: Vec<T>) -> Result<FzfResult<T>> {
        if items.is_empty() {
            return Ok(FzfResult::Cancelled);
        }

        let input_text = Self::prepare_padded_input(&items);
        let preview_dir = Self::prepare_padded_previews(&items)?;

        let mut cmd = self.configure_padded_cmd(&preview_dir);

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
        let _ = std::fs::remove_dir_all(&preview_dir);

        match output {
            Ok(result) => {
                // Handle cancellation (Esc or Ctrl-C)
                if let Some(code) = result.status.code()
                    && (code == 130 || code == 143)
                {
                    return Ok(FzfResult::Cancelled);
                }

                if !result.status.success() {
                    check_for_old_fzf_and_exit(&result.stderr);
                    log_fzf_failure(&result.stderr, result.status.code());
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

        for item in items {
            let display = item.fzf_display_text();
            // Extract background color from display text to create matching padding
            // The icon badge format is: {bg}{fg}  {icon}  {reset} ...
            // We want padding lines to have the same colored block at the start,
            // with a subtle shadow on the bottom padding using a lower block character
            let (top_padding, bottom_with_shadow) = extract_icon_padding(&display);
            // Create padded multi-line item with shadow on bottom
            let padded_item = format!("{top_padding}\n  {display}\n{bottom_with_shadow}");
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
                        let _ = file.write_all(cmd.as_bytes());
                    }
                }
                FzfPreview::None => {}
            }
        }
        Ok(preview_dir)
    }

    fn configure_padded_cmd(&self, preview_dir: &std::path::Path) -> Command {
        let mut cmd = Command::new("fzf");
        cmd.env_remove("FZF_DEFAULT_OPTS");

        // Core options for multi-line items
        cmd.arg("--read0"); // NUL-separated input
        cmd.arg("--ansi"); // ANSI color support
        cmd.arg("--highlight-line"); // Highlight entire multi-line item
        cmd.arg("--layout=reverse");
        cmd.arg("--tiebreak=index");
        cmd.arg("--info=inline-right");

        // Use --bind to print the index on accept instead of the selection text
        // {n} is the 0-based index of the selected item
        cmd.arg("--bind").arg("enter:become(echo {n})");

        // Preview command using {n} for the 0-based item index
        // Check for .sh script first (Command preview), then .txt (Text preview)
        let preview_cmd = format!(
            "if [ -f {dir}/{{n}}.sh ]; then bash {dir}/{{n}}.sh; elif [ -f {dir}/{{n}}.txt ]; then cat {dir}/{{n}}.txt; fi",
            dir = preview_dir.display()
        );
        cmd.arg("--preview").arg(&preview_cmd);

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
        let wrapper = FzfWrapper::new(
            self.multi_select,
            self.prompt,
            self.header,
            self.additional_args,
            self.initial_cursor,
            self.responsive_layout,
        );
        wrapper.select_streaming(command)
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

        if let Some(code) = output.status.code()
            && (code == 130 || code == 143)
        {
            return Ok(FzfResult::Cancelled);
        }

        if !output.status.success() {
            check_for_old_fzf_and_exit(&output.stderr);
            log_fzf_failure(&output.stderr, output.status.code());
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

    fn execute_confirm(self) -> Result<ConfirmResult> {
        let (yes_text, no_text) = if let DialogType::Confirmation {
            ref yes_text,
            ref no_text,
        } = self.dialog_type
        {
            (yes_text.clone(), no_text.clone())
        } else {
            return Ok(ConfirmResult::Cancelled);
        };

        let mut cmd = Command::new("fzf");
        cmd.env_remove("FZF_DEFAULT_OPTS");
        cmd.arg("--layout").arg("reverse");

        if let Some(header) = &self.header {
            cmd.arg("--header").arg(header.to_fzf_string());
        }

        cmd.arg("--prompt").arg("> ");

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

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to open stdin"))?;
        writeln!(stdin, "{yes_text}")?;
        writeln!(stdin, "{no_text}")?;
        stdin.flush()?;

        let output = child.wait_with_output()?;
        crate::menu::server::unregister_menu_process(pid);

        if !output.status.success() {
            check_for_old_fzf_and_exit(&output.stderr);
            log_fzf_failure(&output.stderr, output.status.code());
            return Ok(ConfirmResult::Cancelled);
        }

        let selected_line = std::str::from_utf8(&output.stdout)?.trim();
        if selected_line.is_empty() {
            return Ok(ConfirmResult::Cancelled);
        }

        if selected_line == yes_text {
            Ok(ConfirmResult::Yes)
        } else if selected_line == no_text {
            Ok(ConfirmResult::No)
        } else {
            Ok(ConfirmResult::Cancelled)
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
        let ok_styled = Self::format_styled_button(&ok_text, crate::ui::catppuccin::colors::GREEN);

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

        if let Some(code) = output.status.code()
            && (code == 130 || code == 143)
        {
            return Ok(());
        }

        if !output.status.success() {
            check_for_old_fzf_and_exit(&output.stderr);
        }

        Ok(())
    }

    /// Format a styled button with icon badge sampling from catppuccin colors
    fn format_styled_button(text: &str, color: &str) -> String {
        use crate::ui::catppuccin::{colors, hex_to_ansi_bg, hex_to_ansi_fg};
        use crate::ui::nerd_font::NerdFont;

        let bg = hex_to_ansi_bg(color);
        let fg = hex_to_ansi_fg(colors::CRUST);
        let reset = "\x1b[49;39m";

        // Create the display line with icon badge
        let icon = char::from(NerdFont::Check);
        let display_line = format!("{bg}{fg}   {icon}   {reset}  {text}");

        // Create padding lines with shadow effect
        let (top_padding, bottom_with_shadow) = extract_icon_padding(&display_line);

        // Multi-line padded item
        format!("{top_padding}\n  {display_line}\n{bottom_with_shadow}")
    }

    /// Format a styled message header with title and separator (inspired by PreviewBuilder)
    fn format_message_header(title: Option<&str>, message: Option<&Header>) -> String {
        use crate::ui::catppuccin::{colors, hex_to_ansi_fg};

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

        // Add message if present
        if let Some(header) = message {
            let text_color = hex_to_ansi_fg(colors::TEXT);
            lines.push(format!("{text_color}{}{RESET}", header.to_fzf_string()));
        }

        lines.join("\n")
    }

    fn execute_checklist<T: FzfSelectable + Clone>(self, items: Vec<T>) -> Result<FzfResult<T>> {
        if items.is_empty() {
            return Ok(FzfResult::Cancelled);
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

        // Track cursor position across FZF restarts
        let mut cursor: Option<usize> = None;

        loop {
            // Build key-to-index map and input text
            let mut key_to_index: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            let mut input_lines = Vec::new();

            for (idx, item) in checklist_items.iter().enumerate() {
                let display = item.fzf_display_text();
                let key = item.fzf_key();
                key_to_index.insert(key.clone(), idx);
                input_lines.push(format!("{}\x1f{}", display, key));
            }

            // Add confirm option at the end
            let confirm_display = confirm_item.fzf_display_text();
            let confirm_key = confirm_item.fzf_key();
            input_lines.push(format!("{}\x1f{}", confirm_display, confirm_key));

            let input_text = input_lines.join("\n");

            // Execute FZF with current state and cursor position
            let result = self.run_checklist_fzf(&input_text, &key_to_index, cursor)?;

            match result {
                ChecklistSelection::Cancelled => return Ok(FzfResult::Cancelled),
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
                            ConfirmResult::Yes => return Ok(FzfResult::Cancelled),
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

                    return Ok(FzfResult::MultiSelected(checked));
                }
            }
        }
    }

    fn run_checklist_fzf(
        &self,
        input_text: &str,
        key_to_index: &std::collections::HashMap<String, usize>,
        cursor: Option<usize>,
    ) -> Result<ChecklistSelection> {
        let mut cmd = Command::new("fzf");
        cmd.env_remove("FZF_DEFAULT_OPTS");
        cmd.arg("--ansi");
        cmd.arg("--tiebreak=index");
        cmd.arg("--layout=reverse");
        cmd.arg("--print-query"); // Always print the query, even if no match

        // Use delimiter to separate display from key
        cmd.arg("--delimiter=\x1f").arg("--with-nth=1");

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
        self.parse_checklist_output(output, key_to_index)
    }

    fn parse_checklist_output(
        &self,
        result: std::process::Output,
        key_to_index: &std::collections::HashMap<String, usize>,
    ) -> Result<ChecklistSelection> {
        let exit_code = result.status.code();

        // Handle cancellation (Esc, Ctrl-C)
        if let Some(code) = exit_code
            && (code == 130 || code == 143)
        {
            return Ok(ChecklistSelection::Cancelled);
        }

        // Handle other non-zero exit codes as errors (except code 1 which we handle below)
        if !result.status.success() && exit_code != Some(1) {
            check_for_old_fzf_and_exit(&result.stderr);
            log_fzf_failure(&result.stderr, exit_code);
            return Ok(ChecklistSelection::Cancelled);
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

        // Extract the key from selected line (format: display\x1fkey)
        if let Some(key) = selected.split('\x1f').nth(1) {
            // Check if it's the confirm action
            if key == ChecklistConfirm::confirm_key() {
                return Ok(ChecklistSelection::Confirmed);
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
        args.extend(super::theme::theme_args());
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
