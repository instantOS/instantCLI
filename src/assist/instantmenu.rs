use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

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
        .find(|entry| entry.key().to_string() == first_selection);

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
/// return the selected item's explicit `value`, or `""` when the user cancels
/// or the selection is empty. Shared by the top-level and group menus.
fn run_single_key_menu(prompt: &str, input: &str) -> Result<String> {
    run_single_key_menu_with_command(&mut Command::new("instantmenu"), prompt, input)
}

fn run_single_key_menu_with_command(
    command: &mut Command,
    prompt: &str,
    input: &str,
) -> Result<String> {
    let output = command
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

/// Accept only machine values offered by this menu; empty output is cancellation.
fn validate_selection(selection: String, values: &[String]) -> Result<String> {
    anyhow::ensure!(
        selection.is_empty() || values.contains(&selection),
        "Invalid selection: {}",
        selection
    );
    Ok(selection)
}

/// Show top-level assist options using instantmenu.
fn show_top_level_instantmenu(assists: &[registry::AssistEntry]) -> Result<String> {
    let (options, values) = build_top_level_options(assists);
    let selection = run_single_key_menu("instantASSIST", &options.join("\n"))?;
    validate_selection(selection, &values)
}

fn build_top_level_options(assists: &[registry::AssistEntry]) -> (Vec<String>, Vec<String>) {
    let mut options = Vec::new();
    let mut values = Vec::new();

    for entry in assists {
        // `key=` drives --single-key activation, `value=` is the machine
        // value printed on selection, and `icon=` draws the gutter glyph;
        // the metadata syntax is hidden from the label. The label (shown
        // in the single-key hover prompt) is a compact `key ◆ name`; the
        // full description lives in the help menu (h).
        match entry {
            registry::AssistEntry::Action(action) => {
                let label = format!(
                    "{} {} {}",
                    action.key,
                    NerdFont::Diamond,
                    short_name(action.description)
                );
                values.push(action.key.to_string());
                options.push(format!(
                    "{{key={key} value={key} icon={icon}}} {label}",
                    key = action.key,
                    icon = action.icon,
                ));
            }
            registry::AssistEntry::Group(group) => {
                let label = format!(
                    "{} {} {} →",
                    group.key,
                    NerdFont::Diamond,
                    short_name(group.description)
                );
                values.push(group.key.to_string());
                options.push(format!(
                    "{{key={key} value={key} icon={icon}}} {label}",
                    key = group.key,
                    icon = group.icon,
                ));
            }
        }
    }

    (options, values)
}

/// Show group options using instantmenu with the actions' real registry keys
fn show_group_options_instantmenu(
    group_prefix: &str,
    entries: &[registry::AssistEntry],
) -> Result<()> {
    let (options, values) = build_group_options(group_prefix, entries);

    // Only the synthesized help row means the group has no actions
    if values.len() <= 1 {
        println!("No options available in this group");
        return Ok(());
    }

    let input = options.join("\n");
    let selection = run_single_key_menu(&format!("instantASSIST - {group_prefix}"), &input)?;
    // Validate before registry lookup: a valid chord from another menu is not offered here.
    let chord = validate_selection(selection, &values)?;
    if chord.is_empty() {
        return Ok(());
    }

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
) -> (Vec<String>, Vec<String>) {
    let mut options = Vec::new();
    let mut values = Vec::new();

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
        options.push(format!(
            "{{key={} value={} icon={}}} {}",
            action.key, chord, action.icon, label
        ));
        values.push(chord);
    }

    // Synthesize `h` = help, mirroring the chord navigator and WM exports
    let help_label = format!("h {} Help", NerdFont::Diamond);
    options.push(format!(
        "{{key=h value={group_prefix}h icon={}}} {}",
        NerdFont::Question,
        help_label
    ));
    values.push(format!("{group_prefix}h"));

    (options, values)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_menu(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", script, "fake-instantmenu"]);
        command
    }

    #[test]
    fn top_level_rows_return_registry_keys() {
        let (options, values) = build_top_level_options(registry::ASSISTS);
        assert_eq!(options.len(), registry::ASSISTS.len());
        for ((row, value), entry) in options.iter().zip(&values).zip(registry::ASSISTS) {
            let key = entry.key();
            assert!(row.starts_with(&format!("{{key={key} value={key} icon=")));
            assert_eq!(
                validate_selection(value.clone(), &values).unwrap(),
                key.to_string()
            );
        }
    }

    #[test]
    fn changed_and_duplicate_names_do_not_change_values() {
        let children = registry::find_group_entries("s").expect("s group exists");
        let (original_rows, original_values) = build_group_options("s", children);
        let mut renamed = children.to_vec();
        for entry in &mut renamed {
            if let registry::AssistEntry::Action(action) = entry {
                action.description = "Same name: changed detail";
            }
        }
        let (renamed_rows, renamed_values) = build_group_options("s", &renamed);
        assert_ne!(original_rows, renamed_rows);
        assert_eq!(original_values, renamed_values);
        for value in original_values {
            assert_eq!(
                validate_selection(value.clone(), &renamed_values).unwrap(),
                value
            );
        }

        let (original_rows, original_values) = build_top_level_options(registry::ASSISTS);
        let mut renamed = registry::ASSISTS.to_vec();
        for entry in &mut renamed {
            match entry {
                registry::AssistEntry::Action(action) => action.description = "Same name",
                registry::AssistEntry::Group(group) => group.description = "Same name",
            }
        }
        let (renamed_rows, renamed_values) = build_top_level_options(&renamed);
        assert_ne!(original_rows, renamed_rows);
        assert_eq!(original_values, renamed_values);
        for value in original_values {
            assert_eq!(
                validate_selection(value.clone(), &renamed_values).unwrap(),
                value
            );
        }
    }

    #[test]
    fn single_key_output_uses_values_not_display_labels() {
        let values = vec!["sq".to_string(), "sh".to_string()];
        for (first_label, second_label) in [("Same label", "Same label"), ("Renamed", "Help")] {
            let input =
                format!("{{key=q value=sq}} {first_label}\n{{key=h value=sh}} {second_label}");
            for value in &values {
                let mut command = fake_menu("cat > /dev/null; printf '%s\\n' \"$SELECTION\"");
                command.env("SELECTION", value);
                let selection =
                    run_single_key_menu_with_command(&mut command, "test", &input).unwrap();
                assert_eq!(validate_selection(selection, &values).unwrap(), *value);
            }
            assert!(validate_selection(first_label.to_string(), &values).is_err());
            assert!(validate_selection(second_label.to_string(), &values).is_err());
        }
    }

    #[test]
    fn selection_rejects_unoffered_values_even_if_valid_in_registry() {
        let (_, top_values) = build_top_level_options(registry::ASSISTS);
        let children = registry::find_group_entries("s").expect("s group exists");
        let (_, group_values) = build_group_options("s", children);
        assert!(registry::find_action("sq").is_some());
        assert!(validate_selection("sq".into(), &top_values).is_err());
        assert!(registry::find_action("b").is_some());
        assert!(validate_selection("b".into(), &group_values).is_err());
        assert!(registry::find_action("ia").is_some());
        assert!(validate_selection("ia".into(), &group_values).is_err());
        assert!(registry::contextual_help_path("ih").is_some());
        assert!(validate_selection("ih".into(), &group_values).is_err());
        for invalid in ["q", "s", "sq\nsh", "sq extra", "not a chord"] {
            assert!(validate_selection(invalid.into(), &group_values).is_err());
        }

        // Being in this group's registry is insufficient if the row was not offered.
        let (_, help_only_values) = build_group_options("s", &[]);
        assert!(validate_selection("sq".into(), &help_only_values).is_err());
    }

    #[test]
    fn cancellation_and_empty_output_remain_no_selection() {
        for script in [
            "cat > /dev/null; exit 1",
            "cat > /dev/null; printf 'sq\\n'; exit 1",
            "cat > /dev/null",
        ] {
            let selection = run_single_key_menu_with_command(
                &mut fake_menu(script),
                "test",
                "{key=q value=sq} QR Code Scanner",
            )
            .unwrap();
            assert_eq!(validate_selection(selection, &["sq".into()]).unwrap(), "");
        }
    }

    #[test]
    fn group_menu_keeps_registry_keys_and_synthesizes_help() {
        let children = registry::find_group_entries("s").expect("s group exists");
        let (options, values) = build_group_options("s", children);

        // Actions activate their real registry key: the QR scanner stays on
        // `q` instead of being remapped onto `h` positionally.
        let qr_row = options
            .iter()
            .find(|row| row.contains("QR Code Scanner"))
            .expect("QR row");
        assert!(qr_row.starts_with("{key=q value=sq "));
        assert!(qr_row.contains(&format!("}} q {} QR Code Scanner", NerdFont::Diamond)));
        assert_eq!(validate_selection("sq".into(), &values).unwrap(), "sq");

        // `h` is the synthesized contextual help row, not an action.
        let help_row = options.last().expect("help row");
        assert!(help_row.starts_with("{key=h value=sh "));
        assert!(help_row.ends_with(&format!("h {} Help", NerdFont::Diamond)));
        assert_eq!(validate_selection("sh".into(), &values).unwrap(), "sh");

        // Menu keys are unique and each row declares its returned value.
        let mut keys: Vec<_> = values
            .iter()
            .map(|value| value.strip_prefix('s').unwrap())
            .collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), values.len());
        assert_eq!(options.len(), values.len());
        for (row, value) in options.iter().zip(&values) {
            assert!(row.contains(&format!(" value={value} ")));
        }
    }
}
