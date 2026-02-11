//! Select flow for switching alternatives.

use std::path::Path;

use anyhow::Result;
use colored::Colorize;

use crate::dot::config::DotfileConfig;
use crate::dot::override_config::{DotfileSource, OverrideConfig};
use crate::dot::sources;
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, Header, MenuCursor};
use crate::ui::catppuccin::fzf_mocha_args;
use crate::ui::prelude::*;

use super::apply::{is_safe_to_switch, remove_override, set_alternative};
use super::create_flow::run_create_flow;
use super::flow::{Flow, message_and_continue, message_and_done};
use super::picker::{MenuItem, SourceOption};

pub(crate) fn run_select_flow(path: &Path, display: &str) -> Result<Flow> {
    let config = DotfileConfig::load(None)?;
    let sources = sources::list_sources_for_target(&config, path)?;

    if sources.is_empty() {
        emit(
            Level::Warn,
            "dot.alternative.not_found",
            &format!(
                "{} No sources found for {}. Use --create to add it.",
                char::from(NerdFont::Warning),
                display.yellow()
            ),
            None,
        );
        return Ok(Flow::Cancelled);
    }

    // Check for unnecessary override (1 source but has override)
    let overrides = OverrideConfig::load()?;
    let has_override = overrides.get_override(path).is_some();

    if sources.len() == 1 {
        return handle_single_source(path, display, &sources[0], has_override);
    }

    // Multiple sources - show selection menu
    run_source_selection_menu(path, display, sources, &overrides)
}

fn handle_single_source(
    path: &Path,
    display: &str,
    source: &DotfileSource,
    has_override: bool,
) -> Result<Flow> {
    if has_override {
        // Unnecessary override - offer to remove it
        #[derive(Clone)]
        enum Choice {
            Remove,
            Back,
        }

        impl std::fmt::Display for Choice {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    Choice::Remove => write!(f, "remove"),
                    Choice::Back => write!(f, "back"),
                }
            }
        }

        impl FzfSelectable for Choice {
            fn fzf_display_text(&self) -> String {
                use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored};
                match self {
                    Choice::Remove => format!(
                        "{} Remove unnecessary override",
                        format_icon_colored(NerdFont::Trash, colors::YELLOW)
                    ),
                    Choice::Back => format!("{} Back", format_back_icon()),
                }
            }
            fn fzf_key(&self) -> String {
                self.to_string()
            }
        }

        match FzfWrapper::builder()
            .header(Header::fancy(&format!(
                "{} (1 source, has unnecessary override)",
                display
            )))
            .prompt("Action: ")
            .args(fzf_mocha_args())
            .responsive_layout()
            .select(vec![Choice::Remove, Choice::Back])?
        {
            FzfResult::Selected(Choice::Remove) => {
                let mut overrides = OverrideConfig::load()?;
                overrides.remove_override(path)?;
                return message_and_done(&format!(
                    "Removed override for '{}'\n\nThe file is still tracked at {} / {}",
                    display, source.repo_name, source.subdir_name
                ));
            }
            _ => return Ok(Flow::Cancelled),
        }
    }

    // Normal single source - show info
    emit(
        Level::Info,
        "dot.alternative.single_source",
        &format!(
            "{} {} is sourced from {} / {}",
            char::from(NerdFont::Check),
            display.cyan(),
            source.repo_name.green(),
            source.subdir_name.green()
        ),
        None,
    );

    let config = DotfileConfig::load(None)?;
    let other_dests: Vec<_> = super::discovery::get_destinations(&config)
        .into_iter()
        .filter(|d| d.repo_name != source.repo_name || d.subdir_name != source.subdir_name)
        .collect();

    if other_dests.is_empty() {
        emit(
            Level::Info,
            "dot.alternative.no_other_repos",
            &format!(
                "   No other writable repos. Add one with {}",
                "ins dot repo clone <url>".cyan()
            ),
            None,
        );
    } else {
        emit(
            Level::Info,
            "dot.alternative.hint",
            &format!(
                "   {} To create alternative: {}",
                char::from(NerdFont::Info),
                format!("ins dot alternative {} --create", display).dimmed()
            ),
            None,
        );
    }
    Ok(Flow::Done)
}

fn run_source_selection_menu(
    path: &Path,
    display: &str,
    sources: Vec<DotfileSource>,
    overrides: &OverrideConfig,
) -> Result<Flow> {
    let current = overrides.get_override(path);
    let default_source = super::default_source_for(&sources);
    let mut cursor = MenuCursor::new();

    let items: Vec<SourceOption> = sources
        .into_iter()
        .map(|source| {
            let is_current = current
                .map(|o| o.source_repo == source.repo_name && o.source_subdir == source.subdir_name)
                .unwrap_or(false);
            SourceOption {
                source,
                is_current,
                exists: true,
            }
        })
        .collect();

    if !is_safe_to_switch(path, &items)? {
        return message_and_continue(&format!(
            "Cannot switch {} - file modified.\n\nUse 'ins dot reset {}' first.",
            display, display
        ));
    }

    loop {
        let mut menu: Vec<MenuItem> = items.clone().into_iter().map(MenuItem::Source).collect();

        // Add Create Alternative option
        menu.push(MenuItem::CreateAlternative);

        if current.is_some()
            && let Some(default) = default_source.clone()
        {
            menu.push(MenuItem::RemoveOverride {
                default_source: default,
            });
        }
        menu.push(MenuItem::Back);

        let config = DotfileConfig::load(None)?;
        let mut builder = FzfWrapper::builder()
            .prompt(format!("Select source for {}: ", display))
            .args(fzf_mocha_args())
            .responsive_layout();

        if let Some(index) = cursor.initial_index(&menu) {
            builder = builder.initial_index(index);
        }

        match builder.select(menu.clone())? {
            FzfResult::Selected(MenuItem::Source(item)) => {
                cursor.update(&MenuItem::Source(item.clone()), &menu);
                set_alternative(&config, path, display, &item)?;
                return Ok(Flow::Done);
            }
            FzfResult::Selected(MenuItem::CreateAlternative) => {
                cursor.update(&MenuItem::CreateAlternative, &menu);
                let sources = sources::list_sources_for_target(&config, path)?;
                match run_create_flow(path, display, &sources)? {
                    Flow::Continue => continue,
                    other => return Ok(other),
                }
            }
            FzfResult::Selected(MenuItem::RemoveOverride { default_source }) => {
                cursor.update(
                    &MenuItem::RemoveOverride {
                        default_source: default_source.clone(),
                    },
                    &menu,
                );
                remove_override(&config, path, display, &default_source)?;
                return Ok(Flow::Done);
            }
            FzfResult::Selected(MenuItem::Back) => {
                cursor.update(&MenuItem::Back, &menu);
                return Ok(Flow::Cancelled);
            }
            FzfResult::Cancelled => return Ok(Flow::Cancelled),
            FzfResult::Error(e) => return Err(anyhow::anyhow!("Selection error: {}", e)),
            _ => return Ok(Flow::Cancelled),
        }
    }
}
