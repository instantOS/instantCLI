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

    for assist in registry::ASSISTS {
        println!(
            "  {} {} {}",
            assist.key.to_string().cyan().bold(),
            assist.title.bold(),
            format!("- {}", assist.description).dimmed()
        );
    }

    Ok(())
}

fn run_assist_selector() -> Result<()> {
    let assists = registry::ASSISTS;

    if assists.is_empty() {
        println!("No assists available");
        return Ok(());
    }

    // Build chord specifications
    let chord_specs: Vec<String> = assists
        .iter()
        .map(|assist| {
            format!(
                "{}:{} {}  {}",
                assist.key,
                char::from(assist.icon),
                assist.title,
                assist.description
            )
        })
        .collect();

    // Show chord menu
    let client = client::MenuClient::new();
    client.show()?;
    client.ensure_server_running()?;

    match client.chord(chord_specs) {
        Ok(Some(selected_key)) => {
            let key = selected_key
                .chars()
                .next()
                .ok_or_else(|| anyhow::anyhow!("Invalid key returned"))?;

            let assist = registry::assist_by_key(key)
                .ok_or_else(|| anyhow::anyhow!("Assist not found for key: {}", key))?;

            execute_assist(assist)?;

            Ok(())
        }
        Ok(None) => Ok(()), // Cancelled
        Err(e) => {
            eprintln!("Error showing chord menu: {e}");
            Err(e.into())
        }
    }
}
