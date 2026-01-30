//! Entry points and dispatch for alternative actions.

use std::path::Path;

use anyhow::Result;

use crate::dot::config::Config;
use crate::dot::override_config::find_all_sources;
use crate::dot::utils::resolve_dotfile_path;

use super::action::Action;
use super::browse::{BrowseMode, run_browse_menu};
use super::create_flow::run_create_flow;
use super::direct::{handle_create_direct, handle_set_direct};
use super::discovery::to_display_path;
use super::lists::{list_directory, list_file};
use super::select_flow::run_select_flow;

/// Options for the alternative command.
pub struct AlternativeOptions<'a> {
    pub path: &'a str,
    pub reset: bool,
    pub create: bool,
    pub list: bool,
    pub set: Option<&'a str>,
    pub repo: Option<&'a str>,
    pub subdir: Option<&'a str>,
}

/// Main entry point for the alternative command.
pub fn handle_alternative(config: &Config, opts: AlternativeOptions<'_>) -> Result<()> {
    let action = Action::from_flags(
        opts.reset,
        opts.create,
        opts.list,
        opts.set,
        opts.repo,
        opts.subdir,
    );
    let target_path = resolve_dotfile_path(opts.path)?;
    let display_path = to_display_path(&target_path);

    if target_path.is_dir() {
        return handle_directory(config, &target_path, &display_path, action);
    }

    handle_file(config, &target_path, &display_path, action)
}

fn handle_directory(config: &Config, dir: &Path, display: &str, action: Action) -> Result<()> {
    match action {
        Action::Reset => Err(anyhow::anyhow!(
            "--reset is not supported for directories. Use it with a specific file."
        )),
        Action::SetDirect { .. } => Err(anyhow::anyhow!(
            "--set is not supported for directories. Use it with a specific file."
        )),
        Action::CreateDirect { .. } => Err(anyhow::anyhow!(
            "--create with --repo is not supported for directories. Use it with a specific file."
        )),
        Action::List => list_directory(config, dir, display),
        Action::Select => run_browse_menu(dir, display, BrowseMode::SelectAlternative),
        Action::Create => run_browse_menu(dir, display, BrowseMode::CreateAlternative),
    }
}

fn handle_file(config: &Config, path: &Path, display: &str, action: Action) -> Result<()> {
    match action {
        Action::Reset => super::apply::reset_override(path, display),
        Action::List => {
            let sources = find_all_sources(config, path)?;
            list_file(path, display, &sources)
        }
        Action::Create => {
            let sources = find_all_sources(config, path)?;
            run_create_flow(path, display, &sources)?;
            Ok(())
        }
        Action::Select => {
            run_select_flow(path, display)?;
            Ok(())
        }
        Action::SetDirect { repo, subdir } => {
            handle_set_direct(config, path, display, &repo, subdir.as_deref())
        }
        Action::CreateDirect { repo, subdir } => {
            handle_create_direct(config, path, display, &repo, &subdir)
        }
    }
}
