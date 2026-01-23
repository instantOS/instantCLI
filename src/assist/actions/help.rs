use crate::assist::registry;
use crate::assist::utils;
use crate::common::shell::shell_quote;
use crate::menu_utils::{FzfPreview, FzfResult, FzfSelectable, FzfWrapper, Header};
use crate::ui::catppuccin::{colors, format_icon_colored, fzf_mocha_args, hex_to_ansi_fg};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;
use anyhow::{Result, anyhow};
use std::io::IsTerminal;

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";

#[derive(Clone)]
struct AssistHelpItem {
    display: String,
    key: String,
    preview: FzfPreview,
}

impl FzfSelectable for AssistHelpItem {
    fn fzf_display_text(&self) -> String {
        self.display.clone()
    }

    fn fzf_key(&self) -> String {
        self.key.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview.clone()
    }
}

pub fn show_help() -> Result<()> {
    show_help_for_path("")
}

pub fn show_help_for_path(path: &str) -> Result<()> {
    if !std::io::stdout().is_terminal() {
        return launch_help_in_terminal(path);
    }

    let items = build_help_items(path);
    if items.is_empty() {
        FzfWrapper::message("No assists available at this level.")?;
        return Ok(());
    }

    let header = build_help_header(path);
    let prompt = format!("{} Search", char::from(NerdFont::Search));

    let result = FzfWrapper::builder()
        .prompt(prompt)
        .header(Header::fancy(&header))
        .args(fzf_mocha_args())
        .args(["--no-sort"])
        .responsive_layout()
        .select(items)?;

    if let FzfResult::Error(err) = result {
        return Err(anyhow!(err));
    }

    Ok(())
}

fn launch_help_in_terminal(path: &str) -> Result<()> {
    let binary = utils::current_exe()?;
    let key_sequence = if path.is_empty() {
        "h".to_string()
    } else {
        format!("{}h", path)
    };

    let command = format!(
        "{} assist run {}",
        shell_quote(&binary.to_string_lossy()),
        shell_quote(&key_sequence)
    );
    let script = format!("#!/usr/bin/env bash\n{}\n", command);

    utils::launch_script_in_terminal(&script, "instantCLI Assists Help")
}

fn build_help_header(path: &str) -> String {
    let title = if path.is_empty() {
        "instantCLI Assists".to_string()
    } else {
        format!("instantCLI Assists - {}", path.to_uppercase())
    };
    let tip = if path.is_empty() {
        "Tip: Press $mod+a to enter assist mode"
    } else {
        "Tip: Press 'h' in any mode to see available actions"
    };

    let title_color = hex_to_ansi_fg(colors::MAUVE);
    let tip_color = hex_to_ansi_fg(colors::SUBTEXT0);
    format!("{title_color}{title}{RESET}\n{tip_color}{tip}{RESET}")
}

fn build_help_items(path: &str) -> Vec<AssistHelpItem> {
    let entries = registry::find_group_entries(path).unwrap_or(&[]);
    let mut items = Vec::new();

    for (i, entry) in entries.iter().enumerate() {
        let is_last = i == entries.len().saturating_sub(1);
        append_help_items(&mut items, entry, "", is_last, path);
    }

    items
}

fn append_help_items(
    items: &mut Vec<AssistHelpItem>,
    entry: &registry::AssistEntry,
    prefix: &str,
    is_last: bool,
    key_prefix: &str,
) {
    let connector = if is_last { "└─" } else { "├─" };
    let child_prefix = if is_last { "   " } else { "│  " };

    match entry {
        registry::AssistEntry::Action(action) => {
            let key_chord = format!("{}{}", key_prefix, action.key);
            let display = format_entry_line(
                prefix,
                connector,
                &key_chord,
                action.icon,
                action.description,
                false,
            );
            let preview = build_action_preview(action, &key_chord);
            items.push(AssistHelpItem {
                display,
                key: format!("action:{key_chord}"),
                preview,
            });
        }
        registry::AssistEntry::Group(group) => {
            let key_chord = format!("{}{}", key_prefix, group.key);
            let display = format_entry_line(
                prefix,
                connector,
                &key_chord,
                group.icon,
                group.description,
                true,
            );
            let preview = build_group_preview(group, &key_chord);
            items.push(AssistHelpItem {
                display,
                key: format!("group:{key_chord}"),
                preview,
            });

            let child_key_prefix = format!("{}{}", key_prefix, group.key);
            let child_indent = format!("{}{}", prefix, child_prefix);

            for (i, child) in group.children.iter().enumerate() {
                let is_last_child = i == group.children.len().saturating_sub(1);
                append_help_items(
                    items,
                    child,
                    &child_indent,
                    is_last_child,
                    &child_key_prefix,
                );
            }
        }
    }
}

fn format_entry_line(
    prefix: &str,
    connector: &str,
    key_chord: &str,
    icon: NerdFont,
    description: &str,
    is_group: bool,
) -> String {
    let tree_color = hex_to_ansi_fg(colors::SURFACE1);
    let text_color = hex_to_ansi_fg(colors::TEXT);
    let key_color = if is_group {
        colors::YELLOW
    } else {
        colors::GREEN
    };
    let icon_color = if is_group {
        colors::SAPPHIRE
    } else {
        colors::GREEN
    };

    let tree = format!("{tree_color}{prefix}{connector}{RESET}");
    let key_fg = hex_to_ansi_fg(key_color);
    let key = format!("{BOLD}{key_fg}{key_chord}{RESET}");
    let badge = format_icon_colored(icon, icon_color);
    let desc = if is_group {
        format!("{BOLD}{text_color}{description}{RESET}")
    } else {
        format!("{text_color}{description}{RESET}")
    };

    format!("{tree} {key} {badge}{desc}")
}

fn build_action_preview(action: &registry::AssistAction, key_chord: &str) -> FzfPreview {
    let mut builder = PreviewBuilder::new()
        .header(action.icon, "Assist Action")
        .text(action.description)
        .blank()
        .field("Key chord", key_chord);

    if action.dependencies.is_empty() {
        builder = builder.blank().subtext("No dependencies required");
    } else {
        builder = builder.blank().title(colors::SAPPHIRE, "Dependencies");
        for dep in action.dependencies {
            builder = builder.bullet(&format!("{}", dep.name));
        }
    }

    builder.build()
}

fn build_group_preview(group: &registry::AssistGroup, key_chord: &str) -> FzfPreview {
    let entry_count = group.children.len().to_string();
    let mut builder = PreviewBuilder::new()
        .header(group.icon, "Assist Group")
        .text(group.description)
        .blank()
        .field("Key chord", key_chord)
        .field("Entries", &entry_count);

    let child_lines = build_child_preview_lines(group.children, key_chord);
    if child_lines.is_empty() {
        builder = builder.blank().subtext("No child assists found");
    } else {
        builder = builder.blank().title(colors::SAPPHIRE, "Contains");
        for line in child_lines {
            builder = builder.bullet(&line);
        }
    }

    builder.build()
}

fn build_child_preview_lines(entries: &[registry::AssistEntry], key_prefix: &str) -> Vec<String> {
    let mut lines = Vec::new();

    for entry in entries {
        let key = format!("{}{}", key_prefix, entry.key());
        let label = entry.description();
        match entry {
            registry::AssistEntry::Action(_) => {
                lines.push(format!("{} {}", key, label));
            }
            registry::AssistEntry::Group(_) => {
                lines.push(format!("{} {} (group)", key, label));
            }
        }
    }

    lines
}
