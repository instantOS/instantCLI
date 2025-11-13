use anyhow::{Context, Result};
use clap::Subcommand;
use std::fs::File;
use std::io::{self, Write};

use crate::menu::client;

use super::execute::execute_assist;
use super::registry;

#[derive(Subcommand, Debug, Clone)]
pub enum AssistCommands {
    /// List available assists
    List,
    /// Run an assist by its key sequence
    Run {
        /// Key sequence of the assist to run (e.g., 'c' or 'vn')
        key_sequence: String,
    },
    /// Export assists to Sway config format
    Export {
        /// Output file path (default: stdout)
        #[arg(short = 'f', long = "file")]
        output_path: Option<std::path::PathBuf>,
    },
    /// Set up Sway integration (export config and add include to main config)
    Setup,
}

/// Handle assist command
pub fn dispatch_assist_command(_debug: bool, command: Option<AssistCommands>) -> Result<()> {
    match command {
        None => run_assist_selector(),
        Some(AssistCommands::List) => list_assists(),
        Some(AssistCommands::Run { key_sequence }) => {
            let action = registry::find_action(&key_sequence).ok_or_else(|| {
                anyhow::anyhow!("No assist found for key sequence: {}", key_sequence)
            })?;
            execute_assist(action)
        }
        Some(AssistCommands::Export { output_path }) => export_sway_config(output_path),
        Some(AssistCommands::Setup) => setup_sway_integration(),
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

/// Export assists to Sway config format
///
/// Generates a tree-like mode structure that mirrors the assist hierarchy:
/// - Single-key actions execute immediately and return to default mode
/// - Group keys transition to sub-modes for multi-key chords
/// - Each mode has Escape/Return bindings to exit
///
/// Example: $mod+a → s → f executes fullscreen screenshot
fn export_sway_config(output_path: Option<std::path::PathBuf>) -> Result<()> {
    let mut output_writer: Box<dyn Write> = match &output_path {
        Some(path) => Box::new(File::create(path).context("Failed to create output file")?),
        None => Box::new(io::stdout()),
    };

    // Write header
    writeln!(
        output_writer,
        "# Sway config for instantCLI assists\n# Generated by `ins assist export`\n#\n# Usage: Include this file in your Sway config with:\n#     include ~/.config/instantassist/sway.conf\n"
    )?;

    // Helper function to generate modes recursively for groups
    fn generate_modes<W: Write>(
        output: &mut W,
        entries: &[registry::AssistEntry],
        mode_name: &str,
        prefix: &str,
    ) -> Result<()> {
        writeln!(output, "mode \"{}\" {{", mode_name)?;
        writeln!(output, "    # Exit with Escape or Return")?;
        writeln!(output, "    bindsym Return mode default")?;
        writeln!(output, "    bindsym Escape mode default\n")?;

        for entry in entries {
            match entry {
                registry::AssistEntry::Action(action) => {
                    let key_sequence = format!("{}{}", prefix, action.key);
                    let cmd = format!("ins assist run {}", key_sequence);
                    writeln!(
                        output,
                        "    bindsym {} exec --no-startup-id {}; mode default",
                        action.key, cmd
                    )?;
                }
                registry::AssistEntry::Group(group) => {
                    let sub_mode_name = format!("{}_{}", mode_name, group.key);
                    writeln!(
                        output,
                        "    bindsym {} mode \"{}\"",
                        group.key, sub_mode_name
                    )?;
                }
            }
        }

        writeln!(output, "}}\n")?;

        // Recursively generate modes for groups
        for entry in entries {
            if let registry::AssistEntry::Group(group) = entry {
                let sub_mode_name = format!("{}_{}", mode_name, group.key);
                let new_prefix = format!("{}{}", prefix, group.key);
                generate_modes(output, group.children, &sub_mode_name, &new_prefix)?;
            }
        }

        Ok(())
    }

    // Generate mode for instantassist
    writeln!(output_writer, "# Enter instantassist mode")?;
    writeln!(output_writer, "bindsym $mod+a mode \"instantassist\"\n")?;

    // Generate all modes recursively
    generate_modes(&mut output_writer, registry::ASSISTS, "instantassist", "")?;

    writeln!(output_writer, "# End of instantCLI assists config")?;

    if let Some(path) = output_path {
        println!("Sway config written to: {}", path.display());
    }

    Ok(())
}

/// Set up Sway integration by exporting config and adding include to main config
fn setup_sway_integration() -> Result<()> {
    use std::fs;

    // Determine paths
    let config_dir = dirs::config_dir().context("Unable to determine user config directory")?;
    let sway_config_dir = config_dir.join("sway");
    let main_config_path = sway_config_dir.join("config");
    let instantassist_config_path = sway_config_dir.join("instantassist");

    // Ensure sway config directory exists
    fs::create_dir_all(&sway_config_dir)
        .with_context(|| format!("Failed to create directory: {}", sway_config_dir.display()))?;

    // Export the assist config
    println!(
        "Exporting assists config to {}",
        instantassist_config_path.display()
    );
    export_sway_config(Some(instantassist_config_path.clone()))?;

    // Check if main config exists
    if !main_config_path.exists() {
        anyhow::bail!(
            "Sway config not found at {}\nPlease ensure Sway is installed and configured.",
            main_config_path.display()
        );
    }

    // Read the main config
    let config_content = fs::read_to_string(&main_config_path)
        .with_context(|| format!("Failed to read {}", main_config_path.display()))?;

    // Check if already included
    const MARKER_START: &str = "# BEGIN instantCLI assists integration (managed automatically)";
    const MARKER_END: &str = "# END instantCLI assists integration";

    if config_content.contains(MARKER_START) {
        println!("✓ Sway config already includes instantassist integration");
        println!("  Config file: {}", instantassist_config_path.display());
        return Ok(());
    }

    // Add include line with markers (use tilde notation for home directory)
    let home_dir = dirs::home_dir().context("Unable to determine home directory")?;
    let relative_path = instantassist_config_path
        .strip_prefix(&home_dir)
        .ok()
        .map(|p| format!("~/{}", p.display()))
        .unwrap_or_else(|| instantassist_config_path.display().to_string());

    let include_line = format!("include {}", relative_path);
    let integration_block = format!("\n{}\n{}\n{}\n", MARKER_START, include_line, MARKER_END);

    let new_config_content = format!("{}{}", config_content, integration_block);

    // Write back the config
    fs::write(&main_config_path, new_config_content)
        .with_context(|| format!("Failed to write {}", main_config_path.display()))?;

    println!("✓ Successfully set up Sway integration");
    println!("  Config file: {}", instantassist_config_path.display());
    println!("  Main config: {}", main_config_path.display());
    println!("\nReload Sway config with: swaymsg reload");

    Ok(())
}
