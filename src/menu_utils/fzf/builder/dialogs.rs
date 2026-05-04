use anyhow::{Result, anyhow};
use std::process::{Command, Stdio};

use crate::ui::catppuccin::{colors, hex_to_ansi_bg, hex_to_ansi_fg};
use crate::ui::nerd_font::NerdFont;

use super::shared::{
    FzfCommandOptions, apply_fzf_command_options, base_fzf_command, build_padded_item,
    run_fzf_with_input,
};
use super::{
    ConfirmBuilder, ConfirmOption, FzfBuilder, InputBuilder, MessageBuilder, PasswordBuilder,
};
use crate::menu_utils::fzf::types::{ConfirmResult, FzfResult, Header};
use crate::menu_utils::fzf::utils::get_terminal_dimensions;
use crate::menu_utils::fzf::wrapper::{FzfWrapper, check_fzf_exit};

// ---------------------------------------------------------------------------
// InputBuilder
// ---------------------------------------------------------------------------

impl InputBuilder {
    pub fn input_dialog(self) -> Result<String> {
        match self.input_result()? {
            FzfResult::Selected(s) => Ok(s),
            FzfResult::Cancelled => Ok(String::new()),
            FzfResult::Error(e) => Err(anyhow!(e)),
            _ => Ok(String::new()),
        }
    }

    pub fn input_result(self) -> Result<FzfResult<String>> {
        #[cfg(test)]
        if let Some(resp) = crate::menu_utils::mock::pop_mock() {
            return match resp {
                crate::menu_utils::mock::MockResponse::InputString(s) => Ok(FzfResult::Selected(s)),
                crate::menu_utils::mock::MockResponse::InputCancelled => Ok(FzfResult::Cancelled),
                other => panic!("Mock: expected input response, got {other:?}"),
            };
        }

        let mut cmd = base_fzf_command();
        cmd.arg("--print-query").arg("--no-info");
        apply_fzf_command_options(
            &mut cmd,
            &self.shared,
            FzfCommandOptions {
                prompt_suffix: Some(" "),
                header: None,
                include_additional_args: true,
                cursor: None,
                responsive_layout: false,
            },
        );

        if let Some(query) = &self.shared.initial_query {
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
}

// ---------------------------------------------------------------------------
// PasswordBuilder
// ---------------------------------------------------------------------------

impl PasswordBuilder {
    pub fn password_dialog(self) -> Result<FzfResult<String>> {
        #[cfg(test)]
        if let Some(resp) = crate::menu_utils::mock::pop_mock() {
            return match resp {
                crate::menu_utils::mock::MockResponse::PasswordString(s) => {
                    let _ = self.confirm;
                    Ok(FzfResult::Selected(s))
                }
                crate::menu_utils::mock::MockResponse::PasswordCancelled => {
                    Ok(FzfResult::Cancelled)
                }
                other => panic!("Mock: expected password response, got {other:?}"),
            };
        }

        let confirm = self.confirm;
        let prompt = self.shared.prompt.clone();
        let header_str = self.shared.header.as_ref().map(|h| h.to_fzf_string());

        loop {
            let pass1 = run_password_prompt(prompt.as_deref(), header_str.as_deref())?;

            if !confirm {
                return Ok(pass1);
            }

            let pass1_str = match pass1 {
                FzfResult::Selected(s) => s,
                _ => return Ok(pass1),
            };

            let pass2 = run_password_prompt(Some("Confirm password"), None)?;

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
}

fn run_password_prompt(prompt: Option<&str>, header: Option<&str>) -> Result<FzfResult<String>> {
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
                fallback_password_input(prompt)
            }
        }
        Err(_) => fallback_password_input(prompt),
    }
}

fn fallback_password_input(prompt: Option<&str>) -> Result<FzfResult<String>> {
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

// ---------------------------------------------------------------------------
// ConfirmBuilder
// ---------------------------------------------------------------------------

impl ConfirmBuilder {
    pub fn confirm_dialog(mut self) -> Result<ConfirmResult> {
        #[cfg(test)]
        if let Some(resp) = crate::menu_utils::mock::pop_mock() {
            return match resp {
                crate::menu_utils::mock::MockResponse::ConfirmYes => Ok(ConfirmResult::Yes),
                crate::menu_utils::mock::MockResponse::ConfirmNo => Ok(ConfirmResult::No),
                crate::menu_utils::mock::MockResponse::ConfirmCancelled => {
                    Ok(ConfirmResult::Cancelled)
                }
                other => panic!("Mock: expected confirm response, got {other:?}"),
            };
        }

        let header_text = format_message_header(None, self.shared.header.as_ref());
        if !header_text.is_empty() {
            self.shared.header = Some(Header::Manual(header_text));
        }

        let options = vec![
            ConfirmOption::new(
                self.yes_text.clone(),
                colors::GREEN,
                NerdFont::Check,
                ConfirmResult::Yes,
            ),
            ConfirmOption::new(
                self.no_text.clone(),
                colors::RED,
                NerdFont::Cross,
                ConfirmResult::No,
            ),
        ];

        // Reuse the padded selection terminal on the entry FzfBuilder by
        // constructing one with the prepared shared config.
        let entry = FzfBuilder {
            shared: self.shared,
        };

        match entry.select_padded(options)? {
            FzfResult::Selected(option) => Ok(option.result),
            FzfResult::Cancelled => Ok(ConfirmResult::Cancelled),
            _ => Ok(ConfirmResult::Cancelled),
        }
    }
}

// ---------------------------------------------------------------------------
// MessageBuilder
// ---------------------------------------------------------------------------

impl MessageBuilder {
    pub fn message_dialog(self) -> Result<()> {
        #[cfg(test)]
        if let Some(resp) = crate::menu_utils::mock::pop_mock() {
            return match resp {
                crate::menu_utils::mock::MockResponse::MessageAck => Ok(()),
                other => panic!("Mock: expected message response, got {other:?}"),
            };
        }

        let ok_text = self.ok_text.clone();
        let title = self.title.clone();

        let mut cmd = base_fzf_command();
        cmd.arg("--layout").arg("reverse");
        cmd.arg("--wrap");
        cmd.arg("--read0");
        cmd.arg("--ansi");
        cmd.arg("--highlight-line");

        cmd.arg("--no-input");
        let header_text = format_message_header(title.as_deref(), self.shared.header.as_ref());
        apply_fzf_command_options(
            &mut cmd,
            &self.shared,
            FzfCommandOptions {
                prompt_suffix: None,
                header: (!header_text.is_empty()).then_some(header_text),
                include_additional_args: true,
                cursor: None,
                responsive_layout: false,
            },
        );

        let ok_styled = format_styled_button(&ok_text, colors::GREEN, NerdFont::Check);
        let output = run_fzf_with_input(cmd, ok_styled.as_bytes())?;

        if check_fzf_exit::<()>(&output).is_some() {
            return Ok(());
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Free helpers (formerly assoc fns on FzfBuilder)
// ---------------------------------------------------------------------------

fn format_styled_button(text: &str, color: &str, icon: NerdFont) -> String {
    let bg = hex_to_ansi_bg(color);
    let fg = hex_to_ansi_fg(colors::CRUST);
    let reset = "\x1b[49;39m";

    let icon = char::from(icon);
    let display_line = format!("{bg}{fg}   {icon}   {reset}  {text}");

    build_padded_item(&display_line)
}

pub(super) fn format_message_header(title: Option<&str>, message: Option<&Header>) -> String {
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
        for wrapped_line in wrap_text(&msg_text, wrap_width) {
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

#[cfg(test)]
mod mock_tests {
    use crate::menu_utils::MockQueue;

    #[test]
    fn test_mock_input_returns_canned_string() {
        let _guard = MockQueue::new().input_string("hello world").guard();
        let result = crate::menu_utils::FzfWrapper::builder()
            .prompt("test")
            .input()
            .input_dialog()
            .unwrap();
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_mock_confirm_yes() {
        let _guard = MockQueue::new().confirm_yes().guard();
        let result = crate::menu_utils::FzfWrapper::builder()
            .confirm("Continue?")
            .confirm_dialog()
            .unwrap();
        assert_eq!(result, crate::menu_utils::ConfirmResult::Yes);
    }

    #[test]
    fn test_mock_confirm_no() {
        let _guard = MockQueue::new().confirm_no().guard();
        let result = crate::menu_utils::FzfWrapper::builder()
            .confirm("Continue?")
            .confirm_dialog()
            .unwrap();
        assert_eq!(result, crate::menu_utils::ConfirmResult::No);
    }

    #[test]
    fn test_mock_message_ack() {
        let _guard = MockQueue::new().message_ack().guard();
        let result = crate::menu_utils::FzfWrapper::builder()
            .message("Hello!")
            .message_dialog();
        assert!(result.is_ok());
    }

    #[test]
    fn test_mock_password() {
        let _guard = MockQueue::new().password("secret123").guard();
        let result = crate::menu_utils::FzfWrapper::builder()
            .prompt("Password")
            .password()
            .password_dialog()
            .unwrap();
        match result {
            crate::menu_utils::FzfResult::Selected(s) => assert_eq!(s, "secret123"),
            other => panic!("Expected Selected, got {other:?}"),
        }
    }
}
