use anyhow::{Context, Result};
use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};

use super::registry;
use crate::ui::prelude::NerdFont;

/// Run assist selector using instantmenu with multi-stage key selection
///
/// This approach uses instantmenu in stages:
/// 1. First call shows top-level keys (h, i, b, j, c, a, m, p, q, e, k, s, t, v)
/// 2. If user selects a group key, second call shows options for that group
/// 3. If user selects an action key, execute it directly
pub fn run_assist_selector_instantmenu() -> Result<()> {
    let assists = registry::ASSISTS;

    if assists.is_empty() {
        println!("No assists available");
        return Ok(());
    }

    // Start with top-level selection
    let first_selection = show_top_level_instantmenu(assists)?;

    if first_selection.is_empty() {
        return Ok(()); // User cancelled
    }

    // Check if this is a group or action
    let entry = assists
        .iter()
        .find(|entry| entry.key() == first_selection.chars().next().unwrap());

    match entry {
        Some(registry::AssistEntry::Action(_action)) => {
            // Single-key action, execute it directly
            let action = registry::find_action(&first_selection)
                .ok_or_else(|| anyhow::anyhow!("Assist not found for key: {}", first_selection))?;
            super::execute::execute_assist(action, &first_selection)
        }
        Some(registry::AssistEntry::Group(group)) => {
            // Show group options
            show_group_options_instantmenu(&first_selection, group.children)
        }
        None => {
            anyhow::bail!("Invalid key selection: {}", first_selection);
        }
    }
}

/// Short display name from a registry description. Descriptions follow a
/// `"Name: detail"` shape (e.g. "Help: Show all available assists" →
/// "Help"); the detail is reserved for the help menu so it fits the
/// single-key hover prompt.
fn short_name(description: &str) -> &str {
    description
        .split_once(": ")
        .map_or(description, |(name, _)| name)
}

/// Show top-level assist options using instantmenu
fn show_top_level_instantmenu(assists: &[registry::AssistEntry]) -> Result<String> {
    let mut options = Vec::new();
    let mut label_to_key: HashMap<String, String> = HashMap::new();

    for entry in assists {
        // `key=` drives --single-key activation and `icon=` draws the gutter
        // glyph in the menu row; both are hidden from the label and output.
        // The label (shown in the single-key hover prompt) is a compact
        // `key ◆ name`; the full description lives in the help menu (h).
        match entry {
            registry::AssistEntry::Action(action) => {
                let label = format!(
                    "{} {} {}",
                    action.key,
                    NerdFont::Diamond,
                    short_name(action.description)
                );
                label_to_key.insert(label.clone(), action.key.to_string());
                options.push(format!(
                    "{{key={} icon={}}} {}",
                    action.key, action.icon, label
                ));
            }
            registry::AssistEntry::Group(group) => {
                let label = format!(
                    "{} {} {} →",
                    group.key,
                    NerdFont::Diamond,
                    short_name(group.description)
                );
                label_to_key.insert(label.clone(), group.key.to_string());
                options.push(format!(
                    "{{key={} icon={}}} {}",
                    group.key, group.icon, label
                ));
            }
        }
    }

    let input = options.join("\n");

    let output = Command::new("instantmenu")
        .args([
            "--prompt",
            "instantASSIST", // Prompt text
            "--line-height",
            "32",           // Minimum height of one menu line (C: -h)
            "--single-key", // instantASSIST single-letter mode (C: -ct)
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("Failed to spawn instantmenu")
        .and_then(|mut child| {
            if let Some(stdin) = child.stdin.as_mut() {
                stdin
                    .write_all(input.as_bytes())
                    .context("Failed to write instantmenu input")?;
            }
            child
                .wait_with_output()
                .context("Failed to wait for instantmenu")
        })?;

    // Cancelled or closed: do nothing.
    if !output.status.success() {
        return Ok(String::new());
    }

    let selection = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if selection.is_empty() {
        return Ok(String::new());
    }

    // --single-key prints the item label; look it up in our map
    let selected_key = label_to_key.get(&selection).cloned().unwrap_or_default();

    Ok(selected_key)
}

/// Show group options using instantmenu with single character keys
fn show_group_options_instantmenu(
    group_prefix: &str,
    entries: &[registry::AssistEntry],
) -> Result<()> {
    let mut options = Vec::new();
    let mut label_to_chord: HashMap<String, String> = HashMap::new();

    // Filter only actions from the group
    let actions: Vec<_> = entries
        .iter()
        .filter_map(|entry| match entry {
            registry::AssistEntry::Action(action) => Some(action),
            _ => None,
        })
        .collect();

    if actions.is_empty() {
        println!("No options available in this group");
        return Ok(());
    }

    // Create single character keys (a, b, c, ...) mapped to the actual chords
    for (i, action) in actions.iter().enumerate() {
        let instantmenu_key = char::from(b'a' + i as u8);
        let actual_chord = format!("{}{}", group_prefix, action.key);

        // Compact `key ◆ name` label; the full description lives in help (h)
        let label = format!(
            "{} {} {}",
            instantmenu_key,
            NerdFont::Diamond,
            short_name(action.description)
        );
        label_to_chord.insert(label.clone(), actual_chord);

        options.push(format!(
            "{{key={} icon={}}} {}",
            instantmenu_key, action.icon, label
        ));
    }

    let input = options.join("\n");

    let output = Command::new("instantmenu")
        .args([
            "--prompt",
            &format!("instantASSIST - {}", group_prefix), // Prompt text with group prefix
            "--line-height",
            "32",           // Minimum height of one menu line (C: -h)
            "--single-key", // instantASSIST single-letter mode (C: -ct)
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("Failed to spawn instantmenu")
        .and_then(|mut child| {
            if let Some(stdin) = child.stdin.as_mut() {
                stdin
                    .write_all(input.as_bytes())
                    .context("Failed to write instantmenu input")?;
            }
            child
                .wait_with_output()
                .context("Failed to wait for instantmenu")
        })?;

    // Cancelled or closed: do nothing.
    if !output.status.success() {
        return Ok(());
    }

    let selection = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if selection.is_empty() {
        return Ok(());
    }

    // --single-key prints the item label; look it up in our map
    let actual_chord = label_to_chord
        .get(&selection)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Invalid selection: {}", selection))?;

    // Execute the selected action
    let action = registry::find_action(&actual_chord)
        .ok_or_else(|| anyhow::anyhow!("Assist not found for key: {}", actual_chord))?;

    super::execute::execute_assist(action, &actual_chord)
}
