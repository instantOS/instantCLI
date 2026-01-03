use anyhow::{Context, Result};
use std::io::Write;
use std::process::{Command, Stdio};

use super::registry;

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

/// Show top-level assist options using instantmenu
fn show_top_level_instantmenu(assists: &[registry::AssistEntry]) -> Result<String> {
    let mut options = Vec::new();

    for entry in assists {
        match entry {
            registry::AssistEntry::Action(action) => {
                options.push(format!("{}:{}", action.key, action.description));
            }
            registry::AssistEntry::Group(group) => {
                options.push(format!("{}:{} →", group.key, group.description));
            }
        }
    }

    let input = options.join("\n");

    let output = Command::new("instantmenu")
        .args([
            "-i", // Case insensitive search
            "-p",
            "instantASSIST", // Prompt text
            "-n",            // Case insensitive?
            "-h",
            "32",  // Line height
            "-F",  // Fuzzy search enabled
            "-ct", // Center text
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

    // Extract the key (first character before the colon)
    let selected_key = selection
        .split_once(':')
        .map(|(key, _)| key.trim())
        .unwrap_or(selection.trim())
        .to_string();

    Ok(selected_key)
}

/// Show group options using instantmenu with single character keys
fn show_group_options_instantmenu(
    group_prefix: &str,
    entries: &[registry::AssistEntry],
) -> Result<()> {
    let mut options = Vec::new();
    let mut key_map: Vec<(char, String)> = Vec::new(); // (instantmenu_key, actual_chord)

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

        options.push(format!(
            "{}:{}",
            instantmenu_key,
            format!("{} ({})", action.description, actual_chord)
        ));

        key_map.push((instantmenu_key, actual_chord));
    }

    let input = options.join("\n");

    let output = Command::new("instantmenu")
        .args([
            "-i", // Case insensitive search
            "-p",
            &format!("instantASSIST - {}", group_prefix), // Prompt text with group prefix
            "-n",                                         // Case insensitive?
            "-h",
            "32",  // Line height
            "-F",  // Fuzzy search enabled
            "-ct", // Center text
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

    // Extract the single character key (before the first colon)
    let instantmenu_key = selection.chars().next().unwrap_or('\0');

    // Find the actual chord corresponding to this key
    let actual_chord = key_map
        .iter()
        .find(|(key, _)| *key == instantmenu_key)
        .map(|(_, chord)| chord.clone())
        .ok_or_else(|| anyhow::anyhow!("Invalid selection: {}", instantmenu_key))?;

    // Execute the selected action
    let action = registry::find_action(&actual_chord)
        .ok_or_else(|| anyhow::anyhow!("Assist not found for key: {}", actual_chord))?;

    super::execute::execute_assist(action, &actual_chord)
}
