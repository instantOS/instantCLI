use crate::assist::registry;
use crate::assist::utils;
use anyhow::Result;

pub fn show_help() -> Result<()> {
    // Generate tree representation
    let help_text = generate_help_tree();

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

fn generate_help_tree() -> String {
    use crate::assist::registry::ASSISTS;
    use colored::Colorize;

    let mut output = String::new();

    // Header
    output.push_str(&format!("{}\n\n", "instantCLI Assists".bold().cyan()));
    output.push_str(&format!(
        "{}\n\n",
        "Available key chords and actions:".bold()
    ));

    // Generate tree for each entry
    for (i, entry) in ASSISTS.iter().enumerate() {
        let is_last = i == ASSISTS.len() - 1;
        generate_entry_tree(&mut output, entry, "", is_last, "");
    }

    output.push_str(&format!(
        "\n{}\n",
        "Tip: Press $mod+a to enter assist mode".dimmed()
    ));

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
