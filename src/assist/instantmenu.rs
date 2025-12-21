use anyhow::{Context, Result};
use std::io::Write;
use std::process::{Command, Stdio};

use super::registry;

/// Run assist selector using instantmenu with full chord display
///
/// Instead of trying to handle multi-key chords with multiple instantmenu calls,
/// this approach shows all possible assists with their full key chords upfront,
/// similar to how the sway/i3 config export displays them. Users can see and
/// select any assist directly.
pub fn run_assist_selector_instantmenu() -> Result<()> {
    let assists = registry::ASSISTS;

    if assists.is_empty() {
        println!("No assists available");
        return Ok(());
    }

    // Build all assist options with full key chords
    let options = build_full_assist_list(assists);

    if options.is_empty() {
        println!("No assists available");
        return Ok(());
    }

    let input = options.join("\n");

    let _output = Command::new("instantmenu")
        .args([
            "-i",                    // Case insensitive search
            "-p", "instantASSIST",   // Prompt text
            "-n",                    // Case insensitive?
            "-h", "32",              // Line height
            "-F",                    // Fuzzy search enabled
            "-ct",                   // Center text
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
    if !_output.status.success() {
        return Ok(());
    }

    let selection = String::from_utf8_lossy(&_output.stdout).trim().to_string();
    if selection.is_empty() {
        return Ok(());
    }

    // Extract the key sequence (text before the first colon)
    let selected_key = selection
        .split_once(':')
        .map(|(key, _)| key.trim())
        .unwrap_or(selection.trim())
        .to_string();

    if selected_key.is_empty() {
        return Ok(());
    }

    // Execute the selected action
    let action = registry::find_action(&selected_key)
        .ok_or_else(|| anyhow::anyhow!("Assist not found for key: {}", selected_key))?;

    super::execute::execute_assist(action, &selected_key)
}

/// Build a list of all assists with their full key chords
/// Format: "key:icon description"
fn build_full_assist_list(assists: &[registry::AssistEntry]) -> Vec<String> {
    let mut options = Vec::new();

    fn add_assist_options(
        options: &mut Vec<String>,
        entries: &[registry::AssistEntry],
        prefix: &str,
    ) {
        for entry in entries {
            match entry {
                registry::AssistEntry::Action(action) => {
                    let key = format!("{}{}", prefix, action.key);
                    options.push(format!(
                        "{}:{} {}",
                        key,
                        char::from(action.icon),
                        action.description
                    ));
                }
                registry::AssistEntry::Group(group) => {
                    let group_key = format!("{}{}", prefix, group.key);
                    // Add the group itself (though groups aren't executable)
                    options.push(format!(
                        "{}:{} {} (group)",
                        group_key,
                        char::from(group.icon),
                        group.description
                    ));

                    // Add all children with the group key as prefix
                    add_assist_options(options, group.children, &group_key);
                }
            }
        }
    }

    add_assist_options(&mut options, assists, "");
    options
}

/// Show help for all assists using instantmenu
pub fn show_help_instantmenu() -> Result<()> {
    use colored::Colorize;

    let assists = registry::ASSISTS;

    // Build help entries
    let mut help_entries = Vec::new();

    fn add_help_entries(entries: &[registry::AssistEntry], prefix: &str, help_entries: &mut Vec<String>) {
        for entry in entries {
            match entry {
                registry::AssistEntry::Action(action) => {
                    let key = format!("{}{}", prefix, action.key);
                    let help_text = format!(
                        "{}{} - {}",
                        key.cyan().bold(),
                        char::from(action.icon),
                        action.description
                    );
                    help_entries.push(help_text);
                }
                registry::AssistEntry::Group(group) => {
                    let key = format!("{}{}", prefix, group.key);
                    let group_text = format!(
                        "{}{} {} (group)",
                        key.cyan().bold(),
                        char::from(group.icon),
                        group.description
                    );
                    help_entries.push(group_text);

                    // Add children with increased indentation
                    for child in group.children {
                        match child {
                            registry::AssistEntry::Action(action) => {
                                let full_key = format!("{}{}{}", prefix, group.key, action.key);
                                let help_text = format!(
                                    "  {}{} - {}",
                                    full_key.cyan().bold(),
                                    char::from(action.icon),
                                    action.description
                                );
                                help_entries.push(help_text);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    add_help_entries(assists, "", &mut help_entries);

    let input = help_entries.join("\n");

    let output = Command::new("instantmenu")
        .args([
            "-i", "-p", "instantASSIST - Help", "-n", "-h", "32", "-F", "-ct",
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

    // Just show the help - user can close instantmenu when done
    Ok(())
}