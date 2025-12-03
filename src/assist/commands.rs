use anyhow::{Context, Result};
use clap::Subcommand;
use std::fs::File;
use std::io::{self, Write};

use crate::menu::client;
use crate::ui::prelude::*;

use super::execute::{execute_assist, install_dependencies_for_assist};
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
    #[command(hide = true)]
    /// Install dependencies for an assist key sequence without running it
    InstallDeps {
        /// Key sequence of the assist to ensure dependencies for
        #[arg(long = "key-sequence")]
        key_sequence: String,
    },
    /// Export assists to Window Manager config format
    Export {
        /// Output file path (default: stdout)
        #[arg(short = 'f', long = "file")]
        output_path: Option<std::path::PathBuf>,
        /// Window manager format (sway or i3)
        #[arg(long, default_value = "sway")]
        format: String,
    },
    /// Set up Window Manager integration (export config and add include to main config)
    Setup {
        /// Window manager to setup (sway or i3)
        #[arg(long, default_value = "sway")]
        wm: String,
    },
    #[command(hide = true)]
    /// Set mouse speed (internal use for slider)
    MouseSet {
        /// Speed value (0-100)
        value: i64,
    },
}

/// Handle assist command
pub fn dispatch_assist_command(_debug: bool, command: Option<AssistCommands>) -> Result<()> {
    match command {
        None => run_assist_selector(),
        Some(AssistCommands::List) => list_assists(),
        Some(AssistCommands::MouseSet { value }) => super::actions::mouse::set_mouse_speed(value),
        Some(AssistCommands::Run { key_sequence }) => {
            // Check if this is a help request (ends with 'h')
            if key_sequence.ends_with('h') && key_sequence.len() > 1 {
                let path = &key_sequence[..key_sequence.len() - 1];
                // Verify the path is valid group
                if registry::find_group_entries(path).is_some() {
                    return super::actions::help::show_help_for_path(path);
                }
            }

            let action = registry::find_action(&key_sequence).ok_or_else(|| {
                anyhow::anyhow!("No assist found for key sequence: {}", key_sequence)
            })?;
            execute_assist(action, &key_sequence)
        }
        Some(AssistCommands::InstallDeps { key_sequence }) => {
            let action = registry::find_action(&key_sequence).ok_or_else(|| {
                anyhow::anyhow!("No assist found for key sequence: {}", key_sequence)
            })?;

            if install_dependencies_for_assist(action)? {
                println!("All dependencies satisfied for {}", action.description);
                Ok(())
            } else {
                anyhow::bail!(
                    "Dependencies for '{}' are still missing after installation attempt",
                    action.description
                );
            }
        }
        Some(AssistCommands::Export { output_path, format }) => export_wm_config(output_path, &format),
        Some(AssistCommands::Setup { wm }) => setup_wm_integration(&wm),
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
                    "  {}{} {}",
                    prefix,
                    action.key.to_string().cyan().bold(),
                    action.description.bold(),
                );
            }
            registry::AssistEntry::Group(group) => {
                println!(
                    "  {}{} {}",
                    prefix,
                    group.key.to_string().cyan().bold(),
                    group.description.bold(),
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

            execute_assist(action, &selected_key)?;

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
                    "{}:{} {}",
                    key,
                    char::from(action.icon),
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
                    group.description
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

/// Export assists to Window Manager config format
///
/// Generates a tree-like mode structure that mirrors the assist hierarchy:
/// - Single-key actions execute immediately and return to default mode
/// - Group keys transition to sub-modes for multi-key chords
/// - Each mode has Escape/Return bindings to exit
///
/// Example: $mod+a → s → f executes fullscreen screenshot
fn export_wm_config(output_path: Option<std::path::PathBuf>, wm_name: &str) -> Result<()> {
    let mut output_writer: Box<dyn Write> = match &output_path {
        Some(path) => Box::new(File::create(path).context("Failed to create output file")?),
        None => Box::new(io::stdout()),
    };

    // Write header
    writeln!(
        output_writer,
        "# {} config for instantCLI assists\n# Generated by `ins assist export`\n#\n# Usage: Include this file in your {} config with:\n#     include ~/.config/instantassist/{}.conf\n",
        wm_name, wm_name, wm_name
    )?;

    // Helper function to generate modes recursively for groups
    fn generate_modes<W: Write>(
        output: &mut W,
        entries: &[registry::AssistEntry],
        mode_name: &str,
        prefix: &str,
    ) -> Result<()> {
        let mut available_keys: Vec<char> = entries
            .iter()
            .filter_map(|entry| {
                // Filter out 'h' if it's already handled as a help binding in a submode
                if !prefix.is_empty() {
                    match entry {
                        registry::AssistEntry::Action(action) if action.key == 'h' => None,
                        _ => Some(entry.key()),
                    }
                } else {
                    Some(entry.key())
                }
            })
            .collect();
        available_keys.sort_unstable();
        let keys_hint = if available_keys.is_empty() {
            "".to_string()
        } else {
            let keys_str: Vec<String> = available_keys.iter().map(|c| c.to_string()).collect();
            format!(" (keys: {})", keys_str.join(", "))
        };

        let mode_name_with_hint = if prefix.is_empty() {
            format!("{}{} (h for help)", mode_name, keys_hint)
        } else {
            format!("{}{} (h for help)", mode_name, keys_hint)
        };
        writeln!(output, "mode \"{}\" {{", mode_name_with_hint)?;
        writeln!(output, "    # Exit with Escape or Return")?;
        writeln!(output, "    bindsym Return mode default")?;
        writeln!(output, "    bindsym Escape mode default\n")?;

        // Add help binding if we're in a submode
        if !prefix.is_empty() {
            let help_cmd = format!("ins assist run {}h", prefix);
            writeln!(
                output,
                "    # Show help for this mode\n    bindsym h exec --no-startup-id {}; mode default\n",
                help_cmd
            )?;
        }

        for entry in entries {
            match entry {
                registry::AssistEntry::Action(action) => {
                    // Skip 'h' if we're in a submode (already handled above)
                    if !prefix.is_empty() && action.key == 'h' {
                        continue;
                    }

                    let key_sequence = format!("{}{}", prefix, action.key);
                    let cmd = format!("ins assist run {}", key_sequence);
                    writeln!(
                        output,
                        "    bindsym {} exec --no-startup-id {}; mode default",
                        action.key, cmd
                    )?;
                }
                registry::AssistEntry::Group(group) => {
                    // Collect available keys for the submode
                    let mut sub_keys: Vec<char> = group
                        .children
                        .iter()
                        .filter_map(|child| match child {
                            registry::AssistEntry::Action(action) if action.key == 'h' => None,
                            _ => Some(child.key()),
                        })
                        .collect();
                    sub_keys.sort_unstable();
                    let sub_keys_hint = if sub_keys.is_empty() {
                        "".to_string()
                    } else {
                        let keys_str: Vec<String> =
                            sub_keys.iter().map(|c| c.to_string()).collect();
                        format!(" (keys: {})", keys_str.join(", "))
                    };

                    writeln!(
                        output,
                        "    bindsym {} mode \"{}_{}{} (h for help)\"",
                        group.key, mode_name, group.key, sub_keys_hint
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

    // Collect available keys for the root mode
    let mut root_keys: Vec<char> = registry::ASSISTS.iter().map(|entry| entry.key()).collect();
    root_keys.sort_unstable();
    let root_keys_hint = if root_keys.is_empty() {
        "".to_string()
    } else {
        let keys_str: Vec<String> = root_keys.iter().map(|c| c.to_string()).collect();
        format!(" (keys: {})", keys_str.join(", "))
    };

    writeln!(
        output_writer,
        "bindsym $mod+a mode \"instantassist{} (h for help)\"\n",
        root_keys_hint
    )?;

    // Generate all modes recursively
    generate_modes(&mut output_writer, registry::ASSISTS, "instantassist", "")?;

    writeln!(output_writer, "# End of instantCLI assists config")?;

    if let Some(path) = output_path {
        println!("{} config written to: {}", wm_name, path.display());
    }

    Ok(())
}

/// Set up Window Manager integration by exporting config and adding include to main config
fn setup_wm_integration(wm_name: &str) -> Result<()> {
    use std::collections::hash_map::DefaultHasher;
    use std::fs;
    use std::hash::{Hash, Hasher};

    // Helper to compute hash of a file
    fn hash_file(path: &std::path::Path) -> Result<u64> {
        if !path.exists() {
            return Ok(0); // Return 0 for non-existent files
        }
        let content =
            fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        Ok(hasher.finish())
    }

    // Determine paths
    let config_dir = dirs::config_dir().context("Unable to determine user config directory")?;
    let wm_config_dir = config_dir.join(wm_name);
    let main_config_path = wm_config_dir.join("config");
    let instantassist_config_path = wm_config_dir.join("instantassist");

    // Ensure wm config directory exists
    fs::create_dir_all(&wm_config_dir)
        .with_context(|| format!("Failed to create directory: {}", wm_config_dir.display()))?;

    // Check if main config exists
    if !main_config_path.exists() {
        anyhow::bail!(
            "{} config not found at {}\nPlease ensure {} is installed and configured.",
            wm_name,
            main_config_path.display(),
            wm_name
        );
    }

    // Compute initial hashes
    let initial_main_hash = hash_file(&main_config_path)?;
    let initial_assist_hash = hash_file(&instantassist_config_path)?;

    // Export the assist config
    export_wm_config(Some(instantassist_config_path.clone()), wm_name)?;

    // Read the main config
    let config_content = fs::read_to_string(&main_config_path)
        .with_context(|| format!("Failed to read {}", main_config_path.display()))?;

    // Check if already included
    const MARKER_START: &str = "# BEGIN instantCLI assists integration (managed automatically)";
    const MARKER_END: &str = "# END instantCLI assists integration";

    if !config_content.contains(MARKER_START) {
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
    }

    // Compute final hashes
    let final_main_hash = hash_file(&main_config_path)?;
    let final_assist_hash = hash_file(&instantassist_config_path)?;

    // Check if anything changed
    let main_changed = initial_main_hash != final_main_hash;
    let assist_changed = initial_assist_hash != final_assist_hash;

    if main_changed || assist_changed {
        emit(
            Level::Success,
            "assist.setup.updated",
            &format!("{} {} config updated", char::from(NerdFont::Check), wm_name),
            None,
        );
        emit(
            Level::Info,
            "assist.setup.config",
            &format!("  Config file: {}", instantassist_config_path.display()),
            None,
        );
        emit(
            Level::Info,
            "assist.setup.main",
            &format!("  Main config: {}", main_config_path.display()),
            None,
        );

        // Reload wm
        let reload_cmd = if wm_name == "sway" { "swaymsg" } else { "i3-msg" };
        match std::process::Command::new(reload_cmd).arg("reload").status() {
            Ok(status) if status.success() => {
                emit(
                    Level::Success,
                    "assist.setup.reloaded",
                    &format!("{} {} configuration reloaded", char::from(NerdFont::Sync), wm_name),
                    None,
                );
            }
            Ok(_) => {
                emit(
                    Level::Warn,
                    "assist.setup.reload_failed",
                    &format!(
                        "{} Failed to reload {} ({} returned non-zero exit code)",
                        char::from(NerdFont::Warning),
                        wm_name,
                        reload_cmd
                    ),
                    None,
                );
            }
            Err(e) => {
                emit(
                    Level::Warn,
                    "assist.setup.reload_error",
                    &format!(
                        "{} Could not run {}: {}",
                        char::from(NerdFont::Warning),
                        reload_cmd,
                        e
                    ),
                    None,
                );
            }
        }
    } else {
        emit(
            Level::Info,
            "assist.setup.unchanged",
            &format!(
                "{} {} config unchanged, skipping reload",
                char::from(NerdFont::Check),
                wm_name
            ),
            None,
        );
        emit(
            Level::Info,
            "assist.setup.config",
            &format!("  Config file: {}", instantassist_config_path.display()),
            None,
        );
    }

    Ok(())
}
