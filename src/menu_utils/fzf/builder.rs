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

        if let Some(title) = &title {
            if let Some(header) = &self.header {
                cmd.arg("--header")
                    .arg(format!("{title}\n\n{}", header.to_fzf_string()));
            }
        } else if let Some(header) = &self.header {
            cmd.arg("--header").arg(header.to_fzf_string());
        }

        cmd.arg("--prompt").arg("- ");

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
            stdin.write_all(ok_text.as_bytes())?;
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
}
