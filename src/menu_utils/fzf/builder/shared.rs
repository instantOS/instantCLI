use anyhow::Result;
use std::process::{Command, Output};

use super::SharedConfig;
use crate::menu_utils::fzf::utils::{extract_icon_padding, handle_fzf_spawn_error};

pub(super) use super::super::utils::base_fzf_command;

pub(super) struct FzfCommandOptions {
    pub prompt_suffix: Option<&'static str>,
    pub header: Option<String>,
    pub include_additional_args: bool,
    pub cursor: Option<usize>,
    pub responsive_layout: bool,
}

pub(super) fn apply_fzf_command_options(
    cmd: &mut Command,
    shared: &SharedConfig,
    options: FzfCommandOptions,
) {
    if let Some(prompt_suffix) = options.prompt_suffix
        && let Some(prompt) = &shared.prompt
    {
        cmd.arg("--prompt").arg(format!("{prompt}{prompt_suffix}"));
    }

    if let Some(header) = options.header {
        cmd.arg("--header").arg(header);
    }

    if options.include_additional_args {
        for arg in shared.args() {
            cmd.arg(arg);
        }
    }

    if let Some(index) = options.cursor {
        cmd.arg("--bind").arg(format!("load:pos({})", index + 1));
    }

    if options.responsive_layout && shared.responsive_layout {
        let layout = super::super::utils::get_responsive_layout();
        cmd.arg(layout.preview_window);
        cmd.arg("--margin").arg(layout.margin);
    }
}

pub(super) fn run_fzf_with_input(cmd: Command, input: &[u8]) -> Result<Output> {
    super::super::wrapper::run_fzf_with_input(cmd, input).inspect_err(|error| {
        if let Some(io_error) = error.downcast_ref::<std::io::Error>() {
            handle_fzf_spawn_error(io_error);
        }
    })
}

pub(super) fn build_padded_item(display_line: &str) -> String {
    build_padded_item_from_lines(display_line, &format!("  {display_line}"))
}

pub(super) fn build_padded_item_from_lines(icon_source: &str, middle_line: &str) -> String {
    let (top_padding, bottom_with_shadow) = extract_icon_padding(icon_source);
    format!("{top_padding}\n{middle_line}\n{bottom_with_shadow}")
}
