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

/// Show a single-key instantmenu with `input` (newline-joined options) and
/// return the selected item label, or `""` when the user cancels or the
/// selection is empty. Shared by the top-level and group menus.
fn run_single_key_menu(prompt: &str, input: &str) -> Result<String> {
    let output = Command::new("instantmenu")
        .args([
            "--prompt",
            prompt,
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

    // Cancelled or closed: no selection.
    if !output.status.success() {
        return Ok(String::new());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
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

    // --single-key prints the item label; look it up in our map
    let selection = run_single_key_menu("instantASSIST", &input)?;
    if selection.is_empty() {
        return Ok(String::new());
    }

    Ok(label_to_key.get(&selection).cloned().unwrap_or_default())
}

/// Show group options using instantmenu with the actions' real registry keys
fn show_group_options_instantmenu(
    group_prefix: &str,
    entries: &[registry::AssistEntry],
) -> Result<()> {
    let (options, label_to_chord) = build_group_options(group_prefix, entries);

    // Only the synthesized help row means the group has no actions
    if label_to_chord.len() <= 1 {
        println!("No options available in this group");
        return Ok(());
    }

    let input = options.join("\n");

    // --single-key prints the item label; look it up in our map
    let selection = run_single_key_menu(&format!("instantASSIST - {group_prefix}"), &input)?;
    if selection.is_empty() {
        return Ok(());
    }

    let chord = label_to_chord
        .get(&selection)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Invalid selection: {}", selection))?;

    // `h` opens contextual help for this group, as in every other backend
    if let Some(path) = registry::contextual_help_path(&chord) {
        return super::actions::help::show_help_for_path(path);
    }

    // Execute the selected action
    let action = registry::find_action(&chord)
        .ok_or_else(|| anyhow::anyhow!("Assist not found for key: {}", chord))?;

    super::execute::execute_assist(action, &chord)
}

/// Build the group menu rows: one per action using its real registry key,
/// plus the synthesized `h` contextual-help row. Keys stay in the canonical
/// namespace, so what the menu shows is what `assist run`, the chord
/// navigator, and the WM exports accept.
fn build_group_options(
    group_prefix: &str,
    entries: &[registry::AssistEntry],
) -> (Vec<String>, HashMap<String, String>) {
    let mut options = Vec::new();
    let mut label_to_chord: HashMap<String, String> = HashMap::new();

    // `h` inside a group is reserved for contextual help (registry test
    // enforces this); skip any defensively.
    for action in entries.iter().filter_map(|entry| match entry {
        registry::AssistEntry::Action(action) if action.key != 'h' => Some(action),
        _ => None,
    }) {
        let chord = format!("{}{}", group_prefix, action.key);

        // Compact `key ◆ name` label; the full description lives in help (h)
        let label = format!(
            "{} {} {}",
            action.key,
            NerdFont::Diamond,
            short_name(action.description)
        );
        label_to_chord.insert(label.clone(), chord);

        options.push(format!(
            "{{key={} icon={}}} {}",
            action.key, action.icon, label
        ));
    }

    // Synthesize `h` = help, mirroring the chord navigator and WM exports
    let help_label = format!("h {} Help", NerdFont::Diamond);
    label_to_chord.insert(help_label.clone(), format!("{group_prefix}h"));
    options.push(format!(
        "{{key=h icon={}}} {}",
        NerdFont::Question,
        help_label
    ));

    (options, label_to_chord)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_menu_keeps_registry_keys_and_synthesizes_help() {
        let children = registry::find_group_entries("s").expect("s group exists");
        let (options, label_to_chord) = build_group_options("s", children);

        // Actions activate their real registry key: the QR scanner stays on
        // `q` instead of being remapped onto `h` positionally
        let qr_label = label_to_chord
            .keys()
            .find(|label| label.contains("QR Code Scanner"))
            .expect("QR row");
        assert!(qr_label.starts_with("q "));
        assert_eq!(label_to_chord.get(qr_label), Some(&"sq".to_string()));
        assert!(
            options
                .iter()
                .any(|row| row.starts_with("{key=q ") && row.contains("QR Code Scanner"))
        );

        // `h` is the synthesized contextual help row, not an action
        let help_label = label_to_chord
            .keys()
            .find(|label| label.contains("Help"))
            .expect("help row");
        assert!(help_label.starts_with("h "));
        assert_eq!(label_to_chord.get(help_label), Some(&"sh".to_string()));
        assert!(
            options
                .iter()
                .any(|row| row.starts_with("{key=h ") && row.contains("Help"))
        );

        // Menu keys are unique
        let mut keys: Vec<char> = label_to_chord
            .keys()
            .map(|label| label.chars().next().expect("label starts with key"))
            .collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), label_to_chord.len());
    }
}
