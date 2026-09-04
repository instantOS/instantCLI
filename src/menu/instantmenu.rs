use anyhow::{Context, Result};
use std::io::Write;
use std::process::{Command, Stdio};

use super::SliderSpec;
use super::protocol::SerializableMenuItem;
use crate::menu_utils::ConfirmResult;

fn shell_escape(value: &str) -> String {
    if !value.is_empty()
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '/' | '.' | '_' | '-' | ':' | '+' | '=')
        })
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}

fn shell_command(arguments: &[String]) -> String {
    arguments
        .iter()
        .map(|argument| shell_escape(argument))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Native instantmenu GUI backend for instantCLI dialog commands
pub struct InstantmenuBackend;

impl InstantmenuBackend {
    /// Show confirmation dialog and return Yes, No, or Cancelled
    pub fn confirm(message: &str) -> Result<ConfirmResult> {
        let is_multiline = message.contains('\n');

        let mut cmd = Command::new("instantmenu");
        cmd.arg("--border-width")
            .arg("4")
            .arg("--position")
            .arg("center")
            .arg("--lines")
            .arg("20")
            .arg("--insensitive")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let input_data = if is_multiline {
            cmd.arg("--reject-no-match")
                .arg("--placeholder")
                .arg("confirmation");

            let mut prompt_buf = String::new();
            for line in message.lines() {
                prompt_buf.push_str(&format!("{{heading}} {line}\n"));
            }
            prompt_buf.push_str("{heading} \n{green} yes\n{red} no\n");
            prompt_buf
        } else {
            cmd.arg("--prompt").arg(format!("{message} "));
            "{green} yes\n{red} no\n".to_string()
        };

        let mut child = cmd.spawn().context("Failed to spawn instantmenu")?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(input_data.as_bytes())?;
        }

        let output = child
            .wait_with_output()
            .context("Failed to wait on instantmenu")?;
        if !output.status.success() {
            return Ok(ConfirmResult::Cancelled);
        }

        let response = String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_lowercase();
        if response.contains("yes") {
            Ok(ConfirmResult::Yes)
        } else if response.contains("no") {
            Ok(ConfirmResult::No)
        } else {
            Ok(ConfirmResult::Cancelled)
        }
    }

    /// Show message dialog with an OK button
    pub fn message(title: Option<&str>, message: &str) -> Result<()> {
        let mut cmd = Command::new("instantmenu");
        cmd.arg("--border-width")
            .arg("4")
            .arg("--position")
            .arg("center")
            .arg("--lines")
            .arg("20")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        if let Some(t) = title {
            cmd.arg("--placeholder").arg(t);
        } else {
            cmd.arg("--placeholder")
                .arg(message.lines().next().unwrap_or("Notice"));
        }

        let mut input_data = String::new();
        for line in message.lines() {
            input_data.push_str(&format!("{{heading}} {line}\n"));
        }
        input_data.push_str("{heading} \n{green icon=check} OK\n");

        let mut child = cmd.spawn().context("Failed to spawn instantmenu")?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(input_data.as_bytes())?;
        }

        let _ = child.wait_with_output()?;
        Ok(())
    }

    /// Show text input dialog
    pub fn input(
        prompt: &str,
        placeholder: Option<&str>,
        initial_text: Option<&str>,
    ) -> Result<Option<String>> {
        let mut cmd = Command::new("instantmenu");
        cmd.arg("--input-only")
            .arg("--position")
            .arg("center")
            .arg("--border-width")
            .arg("4")
            .arg("--width")
            .arg("800")
            .arg("--prompt")
            .arg(prompt)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        if let Some(ph) = placeholder {
            cmd.arg("--placeholder").arg(ph);
        }
        if let Some(init) = initial_text {
            cmd.arg("--initial-text").arg(init);
        }

        let mut child = cmd.spawn().context("Failed to spawn instantmenu")?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(b"\n");
        }

        let output = child
            .wait_with_output()
            .context("Failed to wait on instantmenu")?;
        if !output.status.success() {
            return Ok(None);
        }

        let text = String::from_utf8_lossy(&output.stdout)
            .trim_end_matches('\n')
            .to_string();
        Ok(Some(text))
    }

    /// Show password input dialog
    pub fn password(prompt: &str) -> Result<Option<String>> {
        let mut cmd = Command::new("instantmenu");
        cmd.arg("--password")
            .arg("--position")
            .arg("center")
            .arg("--border-width")
            .arg("4")
            .arg("--width")
            .arg("800")
            .arg("--prompt")
            .arg(prompt)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut child = cmd.spawn().context("Failed to spawn instantmenu")?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(b"\n");
        }

        let output = child
            .wait_with_output()
            .context("Failed to wait on instantmenu")?;
        if !output.status.success() {
            return Ok(None);
        }

        let text = String::from_utf8_lossy(&output.stdout)
            .trim_end_matches('\n')
            .to_string();
        Ok(Some(text))
    }

    /// Show choice dialog and return selected item(s)
    ///
    /// With `allow_multiple` the user can confirm additional items with
    /// ctrl+return before finishing with return; every confirmed line is
    /// collected from stdout.
    pub fn choice(
        prompt: &str,
        items: &[SerializableMenuItem],
        allow_multiple: bool,
    ) -> Result<Vec<String>> {
        let mut cmd = Command::new("instantmenu");
        cmd.arg("--border-width")
            .arg("4")
            .arg("--position")
            .arg("center")
            .arg("--width")
            .arg("auto")
            .arg("--lines")
            .arg("20")
            .arg("--insensitive")
            .arg("--prompt")
            .arg(if allow_multiple {
                format!("{prompt} (ctrl+return adds more)")
            } else {
                prompt.to_string()
            })
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut input_data = String::new();
        for item in items {
            input_data.push_str(&item.display_text);
            input_data.push('\n');
        }

        let mut child = cmd.spawn().context("Failed to spawn instantmenu")?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(input_data.as_bytes())?;
        }

        let output = child
            .wait_with_output()
            .context("Failed to wait on instantmenu")?;
        if !output.status.success() {
            return Ok(vec![]);
        }

        let selected = String::from_utf8_lossy(&output.stdout);
        let selected: Vec<String> = selected
            .lines()
            .map(str::trim_end)
            .filter(|line| !line.is_empty())
            .map(ToString::to_string)
            .collect();
        Ok(selected)
    }

    /// Show choice dialog streaming items from stdin.
    ///
    /// Spawns `instantmenu` immediately (it grabs the keyboard and grows
    /// the list as stdin arrives — see `instantmenu --help`) and pumps
    /// stdin lines into it on a background thread. stdin `EOF` closes the
    /// input pipe but leaves the menu open for selection; early menu exit
    /// surfaces as `EPIPE` in the pump, which stops quietly. The pump may
    /// stay blocked on stdin for infinite producers — the short-lived CLI
    /// process exiting kills it, so the handle is detached, not joined.
    pub fn choice_from_stdin_streaming(prompt: &str, allow_multiple: bool) -> Result<Vec<String>> {
        let mut cmd = Command::new("instantmenu");
        cmd.arg("--border-width")
            .arg("4")
            .arg("--position")
            .arg("center")
            .arg("--width")
            .arg("auto")
            .arg("--lines")
            .arg("20")
            .arg("--insensitive")
            .arg("--prompt")
            .arg(if allow_multiple {
                format!("{prompt} (ctrl+return adds more)")
            } else {
                prompt.to_string()
            })
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut child = cmd.spawn().context("Failed to spawn instantmenu")?;
        let child_stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to capture instantmenu stdin"))?;

        std::thread::spawn(move || {
            use std::io::BufRead;
            let mut writer = std::io::BufWriter::new(child_stdin);
            let stdin = std::io::stdin();
            let mut reader = std::io::BufReader::new(stdin.lock());
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        use std::io::Write;
                        if writer.write_all(line.as_bytes()).is_err() || writer.flush().is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        let output = child
            .wait_with_output()
            .context("Failed to wait on instantmenu")?;
        if !output.status.success() {
            return Ok(vec![]);
        }

        let selected = String::from_utf8_lossy(&output.stdout);
        let selected: Vec<String> = selected
            .lines()
            .map(str::trim_end)
            .filter(|line| !line.is_empty())
            .map(ToString::to_string)
            .collect();
        Ok(selected)
    }

    /// Show slider prompt via instantmenu slide
    pub fn slide(spec: &SliderSpec) -> Result<Option<i64>> {
        let mut cmd = Command::new("instantmenu");
        cmd.arg("slide")
            .arg("--min")
            .arg(spec.min.to_string())
            .arg("--max")
            .arg(spec.max.to_string());

        if let Some(v) = spec.value {
            cmd.arg("--value").arg(v.to_string());
        }
        if let Some(s) = spec.step {
            cmd.arg("--step").arg(s.to_string());
        }
        if let Some(bs) = spec.big_step {
            cmd.arg("--big-step").arg(bs.to_string());
        }
        if let Some(ref lbl) = spec.label {
            cmd.arg("--prompt").arg(lbl);
        }
        if !spec.command.is_empty() {
            cmd.arg("--command").arg(shell_command(&spec.command));
        }

        cmd.stdout(Stdio::piped()).stderr(Stdio::null());

        let child = cmd.spawn().context("Failed to spawn instantmenu slide")?;
        let output = child
            .wait_with_output()
            .context("Failed to wait on instantmenu")?;
        if !output.status.success() {
            return Ok(None);
        }

        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if let Ok(val) = text.parse::<i64>() {
            Ok(Some(val))
        } else {
            Ok(None)
        }
    }

    /// Show checklist multi-select dialog
    pub fn checklist(items: &[String], confirm_label: &str) -> Result<Option<Vec<String>>> {
        let mut selected_indices = std::collections::HashSet::new();

        loop {
            let mut input_data = String::new();
            input_data.push_str(&format!("{{green icon=check}} {confirm_label}\n"));
            for (idx, item) in items.iter().enumerate() {
                let checkbox = if selected_indices.contains(&idx) {
                    "{green icon=square-check}"
                } else {
                    "{detail icon=square}"
                };
                input_data.push_str(&format!("{checkbox} {item}\n"));
            }

            let mut cmd = Command::new("instantmenu");
            cmd.arg("--border-width")
                .arg("4")
                .arg("--position")
                .arg("center")
                .arg("--lines")
                .arg("20")
                .arg("--insensitive")
                .arg("--prompt")
                .arg("Select items: ")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null());

            let mut child = cmd.spawn().context("Failed to spawn instantmenu")?;
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(input_data.as_bytes())?;
            }

            let output = child
                .wait_with_output()
                .context("Failed to wait on instantmenu")?;
            if !output.status.success() {
                return Ok(None);
            }

            let raw_choice = String::from_utf8_lossy(&output.stdout)
                .trim_end_matches('\n')
                .to_string();

            if raw_choice == confirm_label || raw_choice.ends_with(confirm_label) {
                let mut result = Vec::new();
                for (idx, item) in items.iter().enumerate() {
                    if selected_indices.contains(&idx) {
                        result.push(item.clone());
                    }
                }
                return Ok(Some(result));
            }

            let mut found = false;
            for (idx, item) in items.iter().enumerate() {
                if raw_choice == *item || raw_choice.ends_with(item) {
                    if selected_indices.contains(&idx) {
                        selected_indices.remove(&idx);
                    } else {
                        selected_indices.insert(idx);
                    }
                    found = true;
                    break;
                }
            }

            if !found {
                return Ok(None);
            }
        }
    }

    /// Show a loading spinner dialog while executing a command, or until stdin is closed
    pub fn spin(message: &str, command: &[String]) -> Result<i32> {
        let input_data = format!("{{heading}} {message}\n{{green icon=hourglass-end}} OK\n");
        let mut cmd = Command::new("instantmenu");
        cmd.arg("--line-height")
            .arg("auto")
            .arg("--lines")
            .arg("20")
            .arg("--position")
            .arg("center")
            .arg("--border-width")
            .arg("4")
            .arg("--width")
            .arg("auto")
            .arg("--placeholder")
            .arg("loading...")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let mut child = cmd
            .spawn()
            .context("Failed to spawn instantmenu for spin")?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(input_data.as_bytes());
        }

        if command.is_empty() {
            use std::io::Read;
            let mut stdin = std::io::stdin();
            let mut buf = [0u8; 128];
            while let Ok(n) = stdin.read(&mut buf) {
                if n == 0 {
                    break;
                }
            }
            let _ = child.kill();
            let _ = child.wait();
            return Ok(0);
        }

        let status = Command::new(&command[0])
            .args(&command[1..])
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status();

        let _ = child.kill();
        let _ = child.wait();

        match status {
            Ok(s) => Ok(s.code().unwrap_or(1)),
            Err(e) => {
                eprintln!("Failed to execute command: {e}");
                Ok(1)
            }
        }
    }

    /// Show an ephemeral toast notification popup
    pub fn toast(message: &str, duration: f64) -> Result<()> {
        let input_data = format!("{{heading}} {message}\n");
        let mut cmd = Command::new("instantmenu");
        cmd.arg("--toast")
            .arg(duration.to_string())
            .arg("--width")
            .arg("auto")
            .arg("--lines")
            .arg("10")
            .arg("--border-width")
            .arg("5")
            .arg("--x-offset")
            .arg("1000000")
            .arg("--y-offset")
            .arg("-1")
            .arg("--placeholder")
            .arg("alert")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let mut child = cmd
            .spawn()
            .context("Failed to spawn instantmenu for toast")?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(input_data.as_bytes());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::shell_command;

    #[test]
    fn slider_command_preserves_argv_boundaries_for_the_shell() {
        let command = vec![
            "sh".to_string(),
            "-c".to_string(),
            "printf '%s\\n' \"$1\"".to_string(),
            "slider command".to_string(),
        ];

        assert_eq!(
            shell_command(&command),
            "sh -c 'printf '\"'\"'%s\\n'\"'\"' \"$1\"' 'slider command'"
        );
    }
}
