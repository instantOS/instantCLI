use anyhow::{Result, anyhow};
use std::io::Write;
use std::process::{Command, Output, Stdio};

use super::FzfBuilder;
use crate::menu_utils::fzf::types::Header;
use crate::menu_utils::fzf::utils::{check_fzf_spawn_error_and_exit, extract_icon_padding};

pub(super) struct FzfCommandOptions {
    pub prompt_suffix: Option<&'static str>,
    pub header: Option<String>,
    pub include_additional_args: bool,
    pub cursor: Option<usize>,
    pub responsive_layout: bool,
}

impl FzfBuilder {
    pub(super) fn base_fzf_command(&self) -> Command {
        let mut cmd = Command::new("fzf");
        cmd.env_remove("FZF_DEFAULT_OPTS");
        cmd
    }

    pub(super) fn apply_fzf_command_options(&self, cmd: &mut Command, options: FzfCommandOptions) {
        if let Some(prompt_suffix) = options.prompt_suffix
            && let Some(prompt) = &self.prompt
        {
            cmd.arg("--prompt").arg(format!("{prompt}{prompt_suffix}"));
        }

        if let Some(header) = options.header {
            cmd.arg("--header").arg(header);
        }

        if options.include_additional_args {
            for arg in &self.additional_args {
                cmd.arg(arg);
            }
        }

        if let Some(index) = options.cursor {
            cmd.arg("--bind").arg(format!("load:pos({})", index + 1));
        }

        if options.responsive_layout && self.responsive_layout {
            let layout = super::super::utils::get_responsive_layout();
            cmd.arg(layout.preview_window);
            cmd.arg("--margin").arg(layout.margin);
        }
    }

    pub(super) fn default_header_text(&self) -> Option<String> {
        self.header.as_ref().map(Header::to_fzf_string)
    }
}

pub(super) fn run_fzf_with_input(mut cmd: Command, input: &[u8]) -> Result<Output> {
    let child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let mut child = match child {
        Ok(child) => child,
        Err(error) => {
            check_fzf_spawn_error_and_exit(&error);
            return Err(anyhow!("fzf execution failed: {error}"));
        }
    };

    let pid = child.id();
    let _ = crate::menu::server::register_menu_process(pid);

    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(input)?;
    }

    let output = child.wait_with_output();
    crate::menu::server::unregister_menu_process(pid);

    match output {
        Ok(output) => Ok(output),
        Err(error) => {
            check_fzf_spawn_error_and_exit(&error);
            Err(anyhow!("fzf execution failed: {error}"))
        }
    }
}

pub(super) fn build_padded_item(display_line: &str) -> String {
    build_padded_item_from_lines(display_line, &format!("  {display_line}"))
}

pub(super) fn build_padded_item_from_lines(icon_source: &str, middle_line: &str) -> String {
    let (top_padding, bottom_with_shadow) = extract_icon_padding(icon_source);
    format!("{top_padding}\n{middle_line}\n{bottom_with_shadow}")
}
