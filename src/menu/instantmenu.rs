use anyhow::{Context, Result};
use std::io::Write;
use std::process::{Command, Stdio};

use super::SliderSpec;
use super::protocol::{ChoiceOptions, InputKind, InputOptions, SerializableMenuItem};
use super::streaming;
use crate::menu_utils::{ConfirmResult, DialogOutcome, FzfSelectable};
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

/// Builder for the `instantmenu` popup flags. Keeps the shared frame flags
/// (`--border-width 4 --position center --lines 20`) and the common flag
/// pairs in one place, and offers a `flags()` seam for tests.
struct InstantmenuCmd {
    args: Vec<String>,
}

impl InstantmenuCmd {
    fn new() -> Self {
        Self { args: Vec::new() }
    }

    /// Frame shared by the centered popup dialogs.
    fn framed() -> Self {
        Self::new().border_width(4).position_center().lines(20)
    }

    fn pair(mut self, flag: &str, value: impl Into<String>) -> Self {
        self.args.push(flag.to_string());
        self.args.push(value.into());
        self
    }

    fn border_width(self, width: u32) -> Self {
        self.pair("--border-width", width.to_string())
    }

    fn position_center(self) -> Self {
        self.pair("--position", "center")
    }

    fn lines(self, count: u32) -> Self {
        self.pair("--lines", count.to_string())
    }

    fn line_height(self, value: &str) -> Self {
        self.pair("--line-height", value)
    }

    fn width(self, value: &str) -> Self {
        self.pair("--width", value)
    }

    fn insensitive(mut self) -> Self {
        self.args.push("--insensitive".to_string());
        self
    }

    fn reject_no_match(mut self) -> Self {
        self.args.push("--reject-no-match".to_string());
        self
    }

    fn prompt(self, text: impl Into<String>) -> Self {
        self.pair("--prompt", text)
    }

    fn placeholder(self, text: impl Into<String>) -> Self {
        self.pair("--placeholder", text)
    }

    fn bind(self, key: &str, label: &str) -> Self {
        self.pair("--bind", format!("{key}:{label}"))
    }

    fn frecency_cache(self, namespace: &str) -> Self {
        self.pair("--frecency-cache", namespace)
    }

    /// The flags built so far (test seam, shared with the keybind path).
    fn flags(&self) -> Vec<String> {
        self.args.clone()
    }

    /// Build a `Command` with the popup stdio used by the blocking dialogs
    /// (piped stdin/stdout, null stderr).
    fn command(&self) -> Command {
        let mut cmd = Command::new("instantmenu");
        cmd.args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        cmd
    }

    /// Spawn, write `input` to stdin, and wait for completion.
    fn spawn_with_input(&self, input: &str) -> Result<std::process::Output> {
        let mut child = self
            .command()
            .spawn()
            .context("Failed to spawn instantmenu")?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(input.as_bytes())?;
        }
        child
            .wait_with_output()
            .context("Failed to wait on instantmenu")
    }
}

/// The canonical flags for a choice dialog: frame, width auto, insensitive,
/// prompt (with the multi-select hint), keybinds, frecency. Shared by the
/// GUI backend and the keybind path so both stay in lockstep.
pub(crate) fn choice_flags(options: &ChoiceOptions) -> Vec<String> {
    let mut cmd =
        InstantmenuCmd::framed()
            .width("auto")
            .insensitive()
            .prompt(if options.allow_multiple {
                format!("{} (ctrl+return adds more)", options.prompt)
            } else {
                options.prompt.clone()
            });
    for binding in &options.bindings {
        cmd = cmd.bind(&binding.key, &binding.label);
    }
    if let Some(namespace) = &options.frecency_cache {
        cmd = cmd.frecency_cache(namespace);
    }
    cmd.flags()
}

/// Native instantmenu GUI backend for instantCLI dialog commands
pub struct InstantmenuBackend;

impl InstantmenuBackend {
    /// Show confirmation dialog and return Yes, No, or Cancelled
    pub fn confirm(message: &str) -> Result<ConfirmResult> {
        let is_multiline = message.contains('\n');

        let mut cmd = InstantmenuCmd::framed().insensitive();

        let input_data = if is_multiline {
            cmd = cmd.reject_no_match().placeholder("confirmation");

            let mut prompt_buf = String::new();
            for line in message.lines() {
                prompt_buf.push_str(&format!("{{heading}} {line}\n"));
            }
            prompt_buf.push_str("{heading} \n{green} yes\n{red} no\n");
            prompt_buf
        } else {
            cmd = cmd.prompt(format!("{message} "));
            "{green} yes\n{red} no\n".to_string()
        };

        let output = cmd.spawn_with_input(&input_data)?;
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
        let placeholder = title.unwrap_or_else(|| message.lines().next().unwrap_or("Notice"));
        let cmd = InstantmenuCmd::framed().placeholder(placeholder);

        let mut input_data = String::new();
        for line in message.lines() {
            input_data.push_str(&format!("{{heading}} {line}\n"));
        }
        input_data.push_str("{heading} \n{green icon=check} OK\n");

        cmd.spawn_with_input(&input_data)?;
        Ok(())
    }

    /// Show text or password input dialog.
    ///
    /// `--placeholder` is forwarded for both text and password input. Upstream
    /// instantmenu renders it while the field is empty (password shows dots
    /// once typed).
    pub(crate) fn input_flags(options: &InputOptions) -> Vec<String> {
        let mut args = Vec::new();
        match &options.kind {
            InputKind::Text { initial_text } => {
                args.push("--input-only".to_string());
                if let Some(text) = initial_text
                    && !text.is_empty()
                {
                    args.push("--initial-text".to_string());
                    args.push(text.clone());
                }
            }
            InputKind::Password => {
                args.push("--password".to_string());
            }
        }
        if let Some(placeholder) = &options.placeholder
            && !placeholder.is_empty()
        {
            args.push("--placeholder".to_string());
            args.push(placeholder.clone());
        }
        args.push("--position".to_string());
        args.push("center".to_string());
        args.push("--border-width".to_string());
        args.push("4".to_string());
        args.push("--width".to_string());
        args.push("800".to_string());
        args.push("--prompt".to_string());
        args.push(options.prompt.clone());
        args
    }

    pub fn input(options: &InputOptions) -> Result<DialogOutcome<String>> {
        let mut cmd = Command::new("instantmenu");
        cmd.args(Self::input_flags(options))
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
            return Ok(DialogOutcome::Cancelled);
        }

        let text = String::from_utf8_lossy(&output.stdout)
            .trim_end_matches(['\r', '\n'])
            .to_string();
        Ok(DialogOutcome::Submitted(text))
    }

    fn choice_command(options: &ChoiceOptions) -> Command {
        let mut cmd = Command::new("instantmenu");
        cmd.args(choice_flags(options))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        cmd
    }

    fn parse_choice_strings(output: std::process::Output) -> Result<DialogOutcome<Vec<String>>> {
        if !output.status.success() {
            return Ok(DialogOutcome::Cancelled);
        }

        let selected = String::from_utf8_lossy(&output.stdout);
        let selected: Vec<String> = selected
            .lines()
            .map(str::trim_end)
            .filter(|line| !line.is_empty())
            .map(ToString::to_string)
            .collect();
        if selected.is_empty() {
            Ok(DialogOutcome::Cancelled)
        } else {
            Ok(DialogOutcome::Submitted(selected))
        }
    }

    /// Show choice dialog and return selected item(s)
    ///
    /// With `allow_multiple` the user can confirm additional items with
    /// ctrl+return before finishing with return; every confirmed line is
    /// collected from stdout.
    pub fn choice(
        options: &ChoiceOptions,
        items: &[SerializableMenuItem],
    ) -> Result<DialogOutcome<Vec<String>>> {
        let mut cmd = Self::choice_command(options);

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
        Self::parse_choice_strings(output)
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
    pub fn choice_from_stdin_streaming(
        options: &ChoiceOptions,
    ) -> Result<DialogOutcome<Vec<String>>> {
        let mut cmd = Self::choice_command(options);

        let mut child = cmd.spawn().context("Failed to spawn instantmenu")?;
        let child_stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to capture instantmenu stdin"))?;

        streaming::spawn_stdin_to_writer_pump(child_stdin);

        let output = child
            .wait_with_output()
            .context("Failed to wait on instantmenu")?;
        Self::parse_choice_strings(output)
    }

    /// Show a choice dialog while typed items arrive from an in-process producer.
    ///
    /// The renderer starts before the producer is drained. The completed item
    /// records are retained here because instantmenu prints their hidden stable
    /// values, while in-process callers may attach metadata needed after selection.
    pub fn choice_streaming(
        options: &ChoiceOptions,
        items: crossbeam_channel::Receiver<SerializableMenuItem>,
    ) -> Result<DialogOutcome<Vec<SerializableMenuItem>>> {
        let mut cmd = Self::choice_command(options);

        let mut child = cmd.spawn().context("Failed to spawn instantmenu")?;
        let child_stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to capture instantmenu stdin"))?;
        let pump = std::thread::spawn(move || {
            let mut writer = std::io::BufWriter::new(child_stdin);
            let mut retained = std::collections::HashMap::new();
            for item in items {
                let key = item.fzf_key();
                let escaped_key = key.replace('\\', "\\\\").replace('"', "\\\"");
                if writeln!(writer, "{{value=\"{escaped_key}\"}} {}", item.display_text).is_err()
                    || writer.flush().is_err()
                {
                    break;
                }
                retained.entry(key).or_insert(item);
            }
            retained
        });

        let output = child
            .wait_with_output()
            .context("Failed to wait on instantmenu")?;
        let retained = pump
            .join()
            .map_err(|_| anyhow::anyhow!("instantmenu item pump panicked"))?;
        if !output.status.success() {
            return Ok(DialogOutcome::Cancelled);
        }

        let mut selected = Vec::new();
        for selected_key in String::from_utf8_lossy(&output.stdout).lines() {
            if let Some(item) = retained.get(selected_key) {
                selected.push(item.clone());
            }
        }
        if selected.is_empty() {
            Ok(DialogOutcome::Cancelled)
        } else {
            Ok(DialogOutcome::Submitted(selected))
        }
    }

    /// Show slider prompt via instantmenu slide
    pub fn slide(spec: &SliderSpec) -> Result<DialogOutcome<i64>> {
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
            return Ok(DialogOutcome::Cancelled);
        }

        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if let Ok(val) = text.parse::<i64>() {
            Ok(DialogOutcome::Submitted(val))
        } else {
            Ok(DialogOutcome::Cancelled)
        }
    }

    /// Show checklist multi-select dialog
    pub fn checklist(items: &[String], confirm_label: &str) -> Result<DialogOutcome<Vec<String>>> {
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

            let cmd = InstantmenuCmd::framed()
                .insensitive()
                .prompt("Select items: ");

            let output = cmd.spawn_with_input(&input_data)?;
            if !output.status.success() {
                return Ok(DialogOutcome::Cancelled);
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
                return Ok(DialogOutcome::Submitted(result));
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
                return Ok(DialogOutcome::Cancelled);
            }
        }
    }

    /// Show a loading spinner dialog while executing a command, or until stdin is closed
    pub fn spin(message: &str, command: &[String]) -> Result<i32> {
        let input_data = format!("{{heading}} {message}\n{{green icon=hourglass-end}} OK\n");
        let flags = InstantmenuCmd::new()
            .line_height("auto")
            .lines(20)
            .position_center()
            .border_width(4)
            .width("auto")
            .placeholder("loading...")
            .flags();
        let mut cmd = Command::new("instantmenu");
        cmd.args(flags)
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
            super::drain_stdin_until_eof();
            let _ = child.kill();
            let _ = child.wait();
            return Ok(0);
        }

        let exit = super::run_spin_command(command);

        let _ = child.kill();
        let _ = child.wait();

        exit
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
    use super::{InstantmenuBackend, InstantmenuCmd, choice_flags, shell_command};
    use crate::menu::protocol::{ChoiceOptions, InputOptions};

    fn has_pair(flags: &[String], key: &str, value: &str) -> bool {
        flags.windows(2).any(|w| w[0] == key && w[1] == value)
    }

    #[test]
    fn choice_flags_matches_the_keybind_path_shape() {
        let options = ChoiceOptions::new("Pick:")
            .multi_select(true)
            .with_frecency_cache(Some("ns".to_string()));
        let flags = choice_flags(&options);

        for (key, value) in [
            ("--border-width", "4"),
            ("--position", "center"),
            ("--width", "auto"),
            ("--lines", "20"),
            ("--frecency-cache", "ns"),
            ("--prompt", "Pick: (ctrl+return adds more)"),
        ] {
            assert!(
                has_pair(&flags, key, value),
                "missing {key} {value}: {flags:?}"
            );
        }
        assert!(flags.contains(&"--insensitive".to_string()));
    }

    #[test]
    fn choice_flags_forwards_bindings() {
        let options =
            ChoiceOptions::new("Pick").with_bindings(vec!["ctrl-e:Edit".parse().unwrap()]);
        let flags = choice_flags(&options);
        assert!(has_pair(&flags, "--bind", "ctrl-e:Edit"), "{flags:?}");
    }

    #[test]
    fn framed_builder_emits_the_shared_popup_frame() {
        let flags = InstantmenuCmd::framed().flags();
        assert_eq!(
            flags,
            vec![
                "--border-width".to_string(),
                "4".to_string(),
                "--position".to_string(),
                "center".to_string(),
                "--lines".to_string(),
                "20".to_string(),
            ]
        );
    }

    #[test]
    fn text_flags_forward_initial_text_and_placeholder() {
        let options = InputOptions::text_with_initial("Edit:", "pre").with_placeholder("hint");
        let flags = InstantmenuBackend::input_flags(&options);
        assert!(flags.contains(&"--input-only".to_string()));
        assert!(has_pair(&flags, "--initial-text", "pre"));
        assert!(has_pair(&flags, "--placeholder", "hint"));
        assert!(has_pair(&flags, "--prompt", "Edit:"));
    }

    #[test]
    fn password_flags_forward_placeholder() {
        let options = InputOptions::password("P:").with_placeholder("hint");
        let flags = InstantmenuBackend::input_flags(&options);
        assert!(flags.contains(&"--password".to_string()));
        assert!(!flags.contains(&"--input-only".to_string()));
        assert!(has_pair(&flags, "--placeholder", "hint"));
    }

    #[test]
    fn empty_prefill_and_placeholder_stay_off_flags() {
        let options = InputOptions::text_with_initial("E:", "").with_placeholder("");
        let flags = InstantmenuBackend::input_flags(&options);
        assert!(!flags.contains(&"--initial-text".to_string()));
        assert!(!flags.contains(&"--placeholder".to_string()));
    }

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
