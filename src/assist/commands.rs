use anyhow::Result;
use clap::Subcommand;

use crate::menu::client;

use super::execute::execute_assist;
use super::registry;

#[derive(Subcommand, Debug, Clone)]
pub enum AssistCommands {
    /// List available assists
    List,
}

/// Handle assist command
pub fn dispatch_assist_command(_debug: bool, command: Option<AssistCommands>) -> Result<()> {
    match command {
        None => run_assist_selector(),
        Some(AssistCommands::List) => list_assists(),
    }
}

fn list_assists() -> Result<()> {
    use colored::Colorize;

    println!("{}", "Available Assists:".bold());
    println!();

    fn print_entry(entry: &registry::AssistEntry, prefix: &str) {
        match entry {
            registry::AssistEntry::Action(action) => {
                println!(
                    "  {}{} {} {}",
                    prefix,
                    action.key.to_string().cyan().bold(),
                    action.title.bold(),
                    format!("- {}", action.description).dimmed()
                );
            }
            registry::AssistEntry::Group(group) => {
                println!(
                    "  {}{} {} {}",
                    prefix,
                    group.key.to_string().cyan().bold(),
                    group.title.bold(),
                    format!("- {}", group.description).dimmed()
                );
                let child_prefix = format!("{}{}  ", prefix, group.key);
                for child in group.children {
                    print_entry(child, &child_prefix);
                }
            }
        }
    }

    for entry in registry::ASSISTS {
        print_entry(entry, "");
    }

    Ok(())
}

fn run_assist_selector() -> Result<()> {
    let assists = registry::ASSISTS;

    if assists.is_empty() {
        println!("No assists available");
        return Ok(());
    }

    // Build chord specifications from the tree structure
    let chord_specs = build_chord_specs(assists);

    // Show chord menu
    let client = client::MenuClient::new();
    client.show()?;
    client.ensure_server_running()?;

    match client.chord(chord_specs) {
        Ok(Some(selected_key)) => {
            let action = registry::find_action(&selected_key)
                .ok_or_else(|| anyhow::anyhow!("Assist not found for key: {}", selected_key))?;

            execute_assist(action)?;

            Ok(())
        }
        Ok(None) => Ok(()), // Cancelled
        Err(e) => {
            eprintln!("Error showing chord menu: {e}");
            Err(e)
        }
    }
}

/// Build chord specifications from the assist tree structure
fn build_chord_specs(entries: &[registry::AssistEntry]) -> Vec<String> {
    let mut specs = Vec::new();

    fn add_entry_specs(specs: &mut Vec<String>, entry: &registry::AssistEntry, prefix: &str) {
        match entry {
            registry::AssistEntry::Action(action) => {
                let key = format!("{}{}", prefix, action.key);
                specs.push(format!(
                    "{}:{} {}  {}",
                    key,
                    char::from(action.icon),
                    action.title,
                    action.description
                ));
            }
            registry::AssistEntry::Group(group) => {
                let key = format!("{}{}", prefix, group.key);

                // Add the group itself
                specs.push(format!(
                    "{}:{} {}",
                    key,
                    char::from(group.icon),
                    group.title
                ));

                // Add all children with the group key as prefix
                for child in group.children {
                    add_entry_specs(specs, child, &key);
                }
            }
        }
    }

    for entry in entries {
        add_entry_specs(&mut specs, entry, "");
    }

    specs
}
