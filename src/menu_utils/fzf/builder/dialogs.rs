use anyhow::{Result, anyhow};
use std::process::{Command, Stdio};

use crate::ui::catppuccin::{colors, hex_to_ansi_bg, hex_to_ansi_fg};
use crate::ui::nerd_font::NerdFont;

use super::shared::{FzfCommandOptions, build_padded_item, run_fzf_with_input};
use super::{ConfirmOption, DialogType, FzfBuilder};
use crate::menu_utils::fzf::types::{ConfirmResult, FzfResult, Header};
use crate::menu_utils::fzf::utils::get_terminal_dimensions;
use crate::menu_utils::fzf::wrapper::{FzfWrapper, check_fzf_exit};

impl FzfBuilder {
    pub(super) fn execute_input(self) -> Result<String> {
        match self.execute_input_result()? {
            FzfResult::Selected(s) => Ok(s),
            FzfResult::Cancelled => Ok(String::new()),
            FzfResult::Error(e) => Err(anyhow!(e)),
            _ => Ok(String::new()),
        }
    }

    pub(super) fn execute_input_result(self) -> Result<FzfResult<String>> {
        let mut cmd = self.base_fzf_command();
        cmd.arg("--print-query").arg("--no-info");
        self.apply_fzf_command_options(
            &mut cmd,
            FzfCommandOptions {
                prompt_suffix: Some(" "),
                header: None,
                include_additional_args: true,
                cursor: None,
                responsive_layout: false,
            },
        );

        if let Some(query) = &self.initial_query {
            cmd.arg("-q").arg(query);
        }

        if let Some(ghost) = &self.ghost_text {
            cmd.arg("--ghost").arg(ghost);
        }

        let output = run_fzf_with_input(cmd, b"")?;

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

    pub(super) fn execute_password(self, confirm: bool) -> Result<FzfResult<String>> {
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

    pub(super) fn execute_confirm(mut self) -> Result<ConfirmResult> {
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

    pub(super) fn execute_message(self) -> Result<()> {
        let (ok_text, title) = if let DialogType::Message {
            ref ok_text,
            ref title,
        } = self.dialog_type
        {
            (ok_text.clone(), title.clone())
        } else {
            return Ok(());
        };

        let mut cmd = self.base_fzf_command();
        cmd.arg("--layout").arg("reverse");
        cmd.arg("--wrap");
        cmd.arg("--read0");
        cmd.arg("--ansi");
        cmd.arg("--highlight-line");

        cmd.arg("--no-input");
        let header_text = Self::format_message_header(title.as_deref(), self.header.as_ref());
        self.apply_fzf_command_options(
            &mut cmd,
            FzfCommandOptions {
                prompt_suffix: None,
                header: (!header_text.is_empty()).then_some(header_text),
                include_additional_args: true,
                cursor: None,
                responsive_layout: false,
            },
        );

        let ok_styled = Self::format_styled_button(&ok_text, colors::GREEN, NerdFont::Check);
        let output = run_fzf_with_input(cmd, ok_styled.as_bytes())?;

        if check_fzf_exit::<()>(&output).is_some() {
            return Ok(());
        }

        Ok(())
    }

    fn format_styled_button(text: &str, color: &str, icon: NerdFont) -> String {
        let bg = hex_to_ansi_bg(color);
        let fg = hex_to_ansi_fg(colors::CRUST);
        let reset = "\x1b[49;39m";

        let icon = char::from(icon);
        let display_line = format!("{bg}{fg}   {icon}   {reset}  {text}");

        build_padded_item(&display_line)
    }

    fn format_message_header(title: Option<&str>, message: Option<&Header>) -> String {
        const RESET: &str = "\x1b[0m";
        const SEPARATOR: &str = "───────────────────────────────────";

        let mut lines = Vec::new();

        if let Some(t) = title {
            let mauve = hex_to_ansi_fg(colors::MAUVE);
            let bold = "\x1b[1m";
            lines.push(format!("{bold}{mauve}{t}{RESET}"));

            let surface = hex_to_ansi_fg(colors::SURFACE1);
            lines.push(format!("{surface}{SEPARATOR}{RESET}"));
        }

        if let Some(header) = message {
            let text_color = hex_to_ansi_fg(colors::TEXT);
            let msg_text = header.to_fzf_string();
            let wrap_width = get_terminal_dimensions()
                .map(|(cols, _)| (cols as usize).saturating_sub(10))
                .unwrap_or(60)
                .max(40);
            for wrapped_line in Self::wrap_text(&msg_text, wrap_width) {
                lines.push(format!("{text_color}{wrapped_line}{RESET}"));
            }
            lines.push(String::new());
            lines.push(String::new());
        }

        lines.join("\n")
    }

    fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
        let mut output_lines = Vec::new();

        for input_line in text.lines() {
            if input_line.is_empty() {
                output_lines.push(String::new());
                continue;
            }

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
}
