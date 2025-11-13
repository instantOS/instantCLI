use crate::assist::registry;
use crate::assist::utils;
use anyhow::Result;

pub fn show_help() -> Result<()> {
    show_help_for_path("")
}

pub fn show_help_for_path(path: &str) -> Result<()> {
    // Generate tree representation
    let help_text = generate_help_tree(path);

    // Show in a terminal window
    let script = format!(
        r#"#!/bin/bash
cat << 'EOF'
{}
EOF
echo ""
echo "Press any key to close..."
read -n 1 -s
"#,
        help_text
    );

    utils::launch_script_in_terminal(&script, "instantCLI Assists Help")?;
    Ok(())
}

fn generate_help_tree(path: &str) -> String {
    use colored::Colorize;

    let mut output = String::new();

    // Get the entries for this path
    let entries = registry::find_group_entries(path).unwrap_or(&[]);

    if entries.is_empty() {
        output.push_str("No assists available at this level.\n");
        return output;
    }

    // Header
    let title = if path.is_empty() {
        "instantCLI Assists".to_string()
    } else {
        format!("instantCLI Assists - {}", path.to_uppercase())
    };
    output.push_str(&format!("{}\n\n", title.bold().cyan()));
    output.push_str(&format!(
        "{}\n\n",
        "Available key chords and actions:".bold()
    ));

    // Generate tree for each entry
    for (i, entry) in entries.iter().enumerate() {
        let is_last = i == entries.len() - 1;
        generate_entry_tree(&mut output, entry, "", is_last, path);
    }

    let tip = if path.is_empty() {
        "Tip: Press $mod+a to enter assist mode"
    } else {
        "Tip: Press 'h' in any mode to see available actions"
    };
    output.push_str(&format!("\n{}\n", tip.dimmed()));

    output
}

fn generate_entry_tree(
    output: &mut String,
    entry: &registry::AssistEntry,
    prefix: &str,
    is_last: bool,
    key_prefix: &str,
) {
    use colored::Colorize;

    let connector = if is_last { "└─" } else { "├─" };
    let child_prefix = if is_last { "   " } else { "│  " };

    match entry {
        registry::AssistEntry::Action(action) => {
            let key_chord = format!("{}{}", key_prefix, action.key);
            output.push_str(&format!(
                "{}{} {} {} - {}\n",
                prefix,
                connector,
                key_chord.green().bold(),
                action.title.bold(),
                action.description.dimmed()
            ));
        }
        registry::AssistEntry::Group(group) => {
            let key_chord = format!("{}{}", key_prefix, group.key);
            output.push_str(&format!(
                "{}{} {} {} {}\n",
                prefix,
                connector,
                key_chord.yellow().bold(),
                group.title.bold(),
                format!("({})", group.description).dimmed()
            ));

            // Generate tree for children
            let child_key_prefix = format!("{}{}", key_prefix, group.key);
            let child_indent = format!("{}{}", prefix, child_prefix);

            for (i, child) in group.children.iter().enumerate() {
                let is_last_child = i == group.children.len() - 1;
                generate_entry_tree(
                    output,
                    child,
                    &child_indent,
                    is_last_child,
                    &child_key_prefix,
                );
            }
        }
    }
}
