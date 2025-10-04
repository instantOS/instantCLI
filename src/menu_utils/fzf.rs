use anyhow::{self, Result};
use base64::{Engine as _, engine::general_purpose};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};

fn shell_escape(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }

    if s.chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '=' | '/' | '.' | ':' | ','))
    {
        return s.to_string();
    }

    format!("'{}'", s.replace('\'', r"'\''"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FzfPreview {
    Text(String),
    Command(String),
    None,
}

pub trait FzfSelectable {
    fn fzf_display_text(&self) -> String;

    fn fzf_preview(&self) -> FzfPreview {
        FzfPreview::None
    }

    fn fzf_key(&self) -> String {
        self.fzf_display_text()
    }
}

impl FzfSelectable for String {
    fn fzf_display_text(&self) -> String {
        self.clone()
    }
}

pub struct FzfWrapper {
    pub(crate) multi_select: bool,
    pub(crate) prompt: Option<String>,
    pub(crate) header: Option<String>,
    pub(crate) additional_args: Vec<String>,
    pub(crate) initial_cursor: Option<InitialCursor>,
}

impl FzfWrapper {
    pub fn builder() -> FzfBuilder {
        FzfBuilder::new()
    }

    fn new(
        multi_select: bool,
        prompt: Option<String>,
        header: Option<String>,
        additional_args: Vec<String>,
        initial_cursor: Option<InitialCursor>,
    ) -> Self {
        Self {
            multi_select,
            prompt,
            header,
            additional_args,
            initial_cursor,
        }
    }

    pub fn select_streaming(&self, input_command: &str) -> Result<FzfResult<String>> {
        let mut fzf_args = vec!["--tiebreak=index".to_string()];

        if self.multi_select {
            fzf_args.push("--multi".to_string());
        }

        if let Some(prompt) = &self.prompt {
            fzf_args.push("--prompt".to_string());
            fzf_args.push(format!("{} > ", prompt));
        }

        if let Some(header) = &self.header {
            fzf_args.push("--header".to_string());
            fzf_args.push(header.clone());
        }

        fzf_args.extend(self.additional_args.clone());

        let mut cmd = Command::new("sh");
        cmd.arg("-c");

        let escaped_args: Vec<String> = fzf_args.iter().map(|arg| shell_escape(arg)).collect();
        let fzf_cmd = format!("fzf {}", escaped_args.join(" "));
        let full_command = format!("{} | {}", input_command, fzf_cmd);
        cmd.arg(&full_command);

        let child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;

        let pid = child.id();
        let _ = crate::menu::server::register_menu_process(pid);

        let output = child.wait_with_output();
        crate::menu::server::unregister_menu_process(pid);

        match output {
            Ok(result) => {
                if let Some(code) = result.status.code()
                    && (code == 130 || code == 143)
                {
                    return Ok(FzfResult::Cancelled);
                }

                let stdout = String::from_utf8_lossy(&result.stdout);
                let selected_lines: Vec<&str> = stdout
                    .trim_end()
                    .split('\n')
                    .filter(|line| !line.is_empty())
                    .collect();

                if selected_lines.is_empty() {
                    Ok(FzfResult::Cancelled)
                } else if self.multi_select {
                    let items: Vec<String> = selected_lines.iter().map(|s| s.to_string()).collect();
                    Ok(FzfResult::MultiSelected(items))
                } else {
                    Ok(FzfResult::Selected(selected_lines[0].to_string()))
                }
            }
            Err(e) => Ok(FzfResult::Error(format!("fzf execution failed: {e}"))),
        }
    }

    pub fn select<T: FzfSelectable + Clone>(&self, items: Vec<T>) -> Result<FzfResult<T>> {
        if items.is_empty() {
            return Ok(FzfResult::Cancelled);
        }

        let mut item_map: HashMap<String, T> = HashMap::new();
        let mut display_lines = Vec::new();

        for item in &items {
            let display = item.fzf_display_text();
            display_lines.push(display.clone());
            item_map.insert(display.clone(), item.clone());
        }

        let cursor_position = match self.initial_cursor.as_ref() {
            Some(InitialCursor::Index(index)) => {
                if display_lines.is_empty() {
                    None
                } else {
                    let idx = *index;
                    let last = display_lines.len() - 1;
                    Some(idx.min(last))
                }
            }
            None => None,
        };

        let preview_map = PreviewUtils::build_preview_mapping(&items)?;
        let has_previews = !preview_map.is_empty();

        let mut cmd = Command::new("fzf");
        cmd.arg("--tiebreak=index");

        if self.multi_select {
            cmd.arg("--multi");
        }

        if let Some(prompt) = &self.prompt {
            cmd.arg("--prompt").arg(format!("{prompt} > "));
        }

        if let Some(header) = &self.header {
            cmd.arg("--header").arg(header);
        }

        if has_previews {
            cmd.arg("--delimiter=\t")
                .arg("--with-nth=1")
                .arg("--preview")
                .arg("echo {} | cut -f2 | base64 -d");
        }

        if let Some(position) = cursor_position {
            cmd.arg("--bind").arg(format!("load:pos({})", position + 1));
        }

        for arg in &self.additional_args {
            cmd.arg(arg);
        }

        let input_text = if has_previews {
            display_lines
                .iter()
                .map(|display| {
                    let preview = preview_map.get(display).cloned().unwrap_or_default();
                    let encoded_preview = general_purpose::STANDARD.encode(preview.as_bytes());
                    format!("{display}\t{encoded_preview}")
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            display_lines.join("\n")
        };

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

        match output {
            Ok(result) => {
                if let Some(code) = result.status.code()
                    && (code == 130 || code == 143)
                {
                    return Ok(FzfResult::Cancelled);
                }

                let stdout = String::from_utf8_lossy(&result.stdout);
                let selected_lines: Vec<&str> = stdout
                    .trim_end()
                    .split('\n')
                    .filter(|line| !line.is_empty())
                    .collect();

                if selected_lines.is_empty() {
                    Ok(FzfResult::Cancelled)
                } else if self.multi_select {
                    let mut selected_items = Vec::new();
                    for line in selected_lines {
                        let display_text = line.split('\t').next().unwrap_or(line);
                        if let Some(item) = item_map.get(display_text).cloned() {
                            selected_items.push(item);
                        }
                    }
                    Ok(FzfResult::MultiSelected(selected_items))
                } else {
                    let display_text = selected_lines[0]
                        .split('\t')
                        .next()
                        .unwrap_or(selected_lines[0]);
                    if let Some(item) = item_map.get(display_text).cloned() {
                        Ok(FzfResult::Selected(item))
                    } else {
                        Ok(FzfResult::Cancelled)
                    }
                }
            }
            Err(e) => Ok(FzfResult::Error(format!("fzf execution failed: {e}"))),
        }
    }

    pub fn select_one<T: FzfSelectable + Clone>(items: Vec<T>) -> Result<Option<T>> {
        match Self::builder().select(items)? {
            FzfResult::Selected(item) => Ok(Some(item)),
            _ => Ok(None),
        }
    }

    pub fn select_many<T: FzfSelectable + Clone>(items: Vec<T>) -> Result<Vec<T>> {
        match Self::builder().multi_select(true).select(items)? {
            FzfResult::MultiSelected(items) => Ok(items),
            FzfResult::Selected(item) => Ok(vec![item]),
            _ => Ok(vec![]),
        }
    }

    pub fn input(prompt: &str) -> Result<String> {
        Self::builder().prompt(prompt).input().input_dialog()
    }

    pub fn message(message: &str) -> Result<()> {
        Self::builder().message(message).message_dialog()
    }

    pub fn confirm(message: &str) -> Result<ConfirmResult> {
        Self::builder().confirm(message).confirm_dialog()
    }

    pub fn input_dialog(prompt: &str) -> Result<String> {
        Self::builder().prompt(prompt).input().input_dialog()
    }

    pub fn password(prompt: &str) -> Result<String> {
        Self::builder().prompt(prompt).password().password_dialog()
    }

    pub fn password_dialog(prompt: &str) -> Result<String> {
        Self::builder().prompt(prompt).password().password_dialog()
    }

    pub fn message_dialog(message: &str) -> Result<()> {
        Self::builder().message(message).message_dialog()
    }

    pub fn confirm_dialog(message: &str) -> Result<ConfirmResult> {
        Self::builder().confirm(message).confirm_dialog()
    }
}

#[derive(Debug)]
pub enum FzfResult<T> {
    Selected(T),
    MultiSelected(Vec<T>),
    Cancelled,
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmResult {
    Yes,
    No,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct FzfBuilder {
    multi_select: bool,
    prompt: Option<String>,
    header: Option<String>,
    additional_args: Vec<String>,
    dialog_type: DialogType,
    initial_cursor: Option<InitialCursor>,
}

#[derive(Debug, Clone)]
enum DialogType {
    Selection,
    Input,
    Password,
    Confirmation {
        yes_text: String,
        no_text: String,
    },
    Message {
        ok_text: String,
        title: Option<String>,
    },
}

#[derive(Debug, Clone)]
enum InitialCursor {
    Index(usize),
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

    pub fn header<S: Into<String>>(mut self, header: S) -> Self {
        self.header = Some(header.into());
        self
    }

    pub fn initial_index(mut self, index: usize) -> Self {
        self.initial_cursor = Some(InitialCursor::Index(index));
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

    pub fn input(mut self) -> Self {
        self.dialog_type = DialogType::Input;
        self.additional_args = Self::input_args();
        self
    }

    pub fn password(mut self) -> Self {
        self.dialog_type = DialogType::Password;
        self.additional_args = Self::password_args();
        self
    }

    pub fn confirm<S: Into<String>>(mut self, message: S) -> Self {
        self.dialog_type = DialogType::Confirmation {
            yes_text: "Yes".to_string(),
            no_text: "No".to_string(),
        };
        self.header = Some(message.into());
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
        self.header = Some(message.into());
        self.additional_args = Self::confirm_args();
        self
    }

    pub fn ok_text<S: Into<String>>(mut self, text: S) -> Self {
        if let DialogType::Message { ok_text, .. } = &mut self.dialog_type {
            *ok_text = text.into();
        }
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
        );
        wrapper.select(items)
    }

    pub fn select_streaming(self, command: &str) -> Result<FzfResult<String>> {
        let wrapper = FzfWrapper::new(
            self.multi_select,
            self.prompt,
            self.header,
            self.additional_args,
            self.initial_cursor,
        );
        wrapper.select_streaming(command)
    }

    pub fn input_dialog(self) -> Result<String> {
        if !matches!(self.dialog_type, DialogType::Input) {
            return Err(anyhow::anyhow!("Builder not configured for input"));
        }
        self.execute_input()
    }

    pub fn password_dialog(self) -> Result<String> {
        if !matches!(self.dialog_type, DialogType::Password) {
            return Err(anyhow::anyhow!("Builder not configured for password"));
        }
        self.execute_password()
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

    pub fn show_selection<T: FzfSelectable + Clone>(self, items: Vec<T>) -> Result<FzfResult<T>> {
        self.select(items)
    }

    pub fn show_input(self) -> Result<String> {
        self.input_dialog()
    }

    pub fn show_password(self) -> Result<String> {
        self.password_dialog()
    }

    pub fn show_confirmation(self) -> Result<ConfirmResult> {
        self.confirm_dialog()
    }

    pub fn show_message(self) -> Result<()> {
        self.message_dialog()
    }

    fn execute_input(self) -> Result<String> {
        let mut cmd = Command::new("fzf");
        cmd.arg("--print-query").arg("--no-info");

        if let Some(prompt) = &self.prompt {
            cmd.arg("--prompt").arg(format!("{prompt} "));
        }

        for arg in self.additional_args {
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
            return Ok(String::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.trim_end().split('\n').collect();

        if let Some(query) = lines.first() {
            Ok(query.trim().to_string())
        } else {
            Ok(String::new())
        }
    }

    fn execute_password(self) -> Result<String> {
        let mut cmd = Command::new("gum");
        cmd.arg("input").arg("--password");

        if let Some(prompt) = &self.prompt {
            cmd.arg("--prompt").arg(format!("{prompt} "));
        }

        cmd.arg("--padding").arg("1 2");
        cmd.arg("--width").arg("60");

        if let Some(header) = &self.header {
            cmd.arg("--placeholder").arg(header);
        } else {
            cmd.arg("--placeholder").arg("Enter your password");
        }

        let child = cmd
            .stdin(Stdio::inherit())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;

        let output = child.wait_with_output()?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            Ok(stdout.trim().to_string())
        } else {
            self.fallback_password_input()
        }
    }

    fn fallback_password_input(&self) -> Result<String> {
        use std::io::Write as _;

        eprint!("{}: ", self.prompt.as_deref().unwrap_or("Enter password"));
        let _ = std::io::stderr().flush();

        let mut password = String::new();
        std::io::stdin().read_line(&mut password)?;

        Ok(password.trim().to_string())
    }

    fn execute_confirm(self) -> Result<ConfirmResult> {
        let (yes_text, no_text) =
            if let DialogType::Confirmation { yes_text, no_text } = self.dialog_type {
                (yes_text, no_text)
            } else {
                return Ok(ConfirmResult::Cancelled);
            };

        let mut cmd = Command::new("fzf");
        cmd.arg("--layout").arg("reverse");

        if let Some(header) = &self.header {
            cmd.arg("--header").arg(format!("{header}\n"));
        }

        cmd.arg("--prompt").arg("> ");

        for arg in self.additional_args {
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
        let (ok_text, title) = if let DialogType::Message { ok_text, title } = self.dialog_type {
            (ok_text, title)
        } else {
            return Ok(());
        };

        let mut cmd = Command::new("fzf");
        cmd.arg("--layout").arg("reverse");

        if let Some(title) = &title {
            if let Some(header) = &self.header {
                cmd.arg("--header").arg(format!("{title}\n\n{header}"));
            }
        } else if let Some(header) = &self.header {
            cmd.arg("--header").arg(header);
        }

        cmd.arg("--prompt").arg("- ");

        for arg in self.additional_args {
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

        Ok(())
    }

    fn default_args() -> Vec<String> {
        let mut args = vec![
            "--margin".to_string(),
            "10%,2%".to_string(),
            "--min-height".to_string(),
            "10".to_string(),
        ];
        args.extend(Self::theme_args());
        args
    }

    fn input_args() -> Vec<String> {
        let mut args = vec![
            "--margin".to_string(),
            "20%,2%".to_string(),
            "--min-height".to_string(),
            "10".to_string(),
        ];
        args.extend(Self::theme_args());
        args
    }

    fn confirm_args() -> Vec<String> {
        let mut args = vec![
            "--margin".to_string(),
            "20%,2%".to_string(),
            "--min-height".to_string(),
            "10".to_string(),
        ];
        args.extend(Self::theme_args());
        args.push("--info=hidden".to_string());
        args.push("--color=header:-1".to_string());
        args
    }

    fn password_args() -> Vec<String> {
        vec![]
    }

    fn theme_args() -> Vec<String> {
        vec![
            "--color=bg+:#313244".to_string(),
            "--color=bg:#1E1E2E".to_string(),
            "--color=spinner:#F5E0DC".to_string(),
            "--color=hl:#F38BA8".to_string(),
            "--color=fg:#CDD6F4".to_string(),
            "--color=header:#F38BA8".to_string(),
            "--color=info:#CBA6F7".to_string(),
            "--color=pointer:#F5E0DC".to_string(),
            "--color=marker:#B4BEFE".to_string(),
            "--color=fg+:#CDD6F4".to_string(),
            "--color=prompt:#CBA6F7".to_string(),
            "--color=hl+:#F38BA8".to_string(),
            "--color=selected-bg:#45475A".to_string(),
            "--color=border:#6C7086".to_string(),
            "--color=label:#CDD6F4".to_string(),
        ]
    }
}

#[derive(Serialize, Deserialize)]
struct PreviewData {
    key: String,
    preview_type: String,
    preview_content: String,
}

pub struct PreviewUtils;

impl PreviewUtils {
    pub fn build_preview_mapping<T: FzfSelectable>(items: &[T]) -> Result<HashMap<String, String>> {
        let mut preview_map = HashMap::new();

        for item in items {
            let display_text = item.fzf_display_text();
            match item.fzf_preview() {
                FzfPreview::Text(text) => {
                    preview_map.insert(display_text, text);
                }
                FzfPreview::Command(cmd) => {
                    preview_map.insert(display_text, cmd);
                }
                FzfPreview::None => {}
            }
        }

        Ok(preview_map)
    }
}
