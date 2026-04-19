use anyhow::{Context, Result};
use clap::Subcommand;
use clap_complete::engine::ArgValueCompleter;
use std::fs::File;
use std::io::{self, Write};

use crate::menu::client;

use super::execute::{execute_assist, install_dependencies_for_assist};
use super::registry;

#[derive(Subcommand, Debug, Clone)]
pub enum AssistCommands {
    /// List available assists
    List,
    /// Run an assist by its key sequence
    Run {
        /// Key sequence of the assist to run (e.g., 'c' or 'vn')
        #[arg(add = ArgValueCompleter::new(crate::completions::assist_key_sequence_completion))]
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
        /// Output file path (default: stdout for i3/sway, ~/.config/instantwm/assist.toml for instantwm)
        #[arg(short = 'f', long = "file")]
        output_path: Option<std::path::PathBuf>,
        /// Window manager format (sway, i3, or instantwm)
        #[arg(long, default_value = "sway")]
        format: String,
    },
    /// Adjust volume directly (+, -, mute, or absolute percentage)
    Volume {
        /// Action: "+", "-", "mute", or a number (0-100)
        action: String,
    },
    /// Adjust brightness directly (+, -, or absolute percentage)
    Bright {
        /// Action: "+", "-", or a number (0-100)
        action: String,
    },
    #[command(hide = true)]
    /// Set mouse speed (internal use for slider)
    MouseSet {
        /// Speed value (0-100)
        value: i64,
    },
    #[command(hide = true)]
    /// Set brightness (internal use for slider)
    BrightnessSet {
        /// Brightness percentage (0-100)
        value: i64,
    },
    #[command(hide = true)]
    /// Set scroll factor (internal use for slider)
    ScrollFactorSet {
        /// Scroll factor value (0-300)
        value: i64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssistInternalCommand {
    MouseSet,
    BrightnessSet,
    ScrollFactorSet,
}

impl AssistInternalCommand {
    pub fn as_str(self) -> &'static str {
        match self {
            AssistInternalCommand::MouseSet => "mouse-set",
            AssistInternalCommand::BrightnessSet => "brightness-set",
            AssistInternalCommand::ScrollFactorSet => "scroll-factor-set",
        }
    }
}

pub fn assist_command_argv(command: AssistInternalCommand) -> Result<Vec<String>> {
    let exe = std::env::current_exe()?;
    Ok(vec![
        exe.to_string_lossy().to_string(),
        "assist".to_string(),
        command.as_str().to_string(),
    ])
}

/// Handle assist command
pub fn dispatch_assist_command(
    _debug: bool,
    use_instantmenu: bool,
    command: Option<AssistCommands>,
) -> Result<()> {
    match command {
        None => run_assist_selector(use_instantmenu),
        Some(AssistCommands::List) => list_assists(),
        Some(AssistCommands::MouseSet { value }) => super::actions::mouse::set_mouse_speed(value),
        Some(AssistCommands::BrightnessSet { value }) => {
            crate::settings::definitions::brightness::set_brightness(value)
        }
        Some(AssistCommands::ScrollFactorSet { value }) => {
            crate::assist::actions::mouse::set_scroll_factor(value)
        }
        Some(AssistCommands::Volume { action }) => super::actions::system::volume_direct(&action),
        Some(AssistCommands::Bright { action }) => super::actions::system::brightness_direct(&action),
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

            if install_dependencies_for_assist(action)?.is_available() {
                println!("All dependencies satisfied for {}", action.description);
                Ok(())
            } else {
                anyhow::bail!(
                    "Dependencies for '{}' are still missing after installation attempt",
                    action.description
                );
            }
        }
        Some(AssistCommands::Export {
            output_path,
            format,
        }) => match format.as_str() {
            "sway" | "i3" => export_wm_config(output_path, &format),
            "instantwm" => export_instantwm_config(output_path),
            _ => anyhow::bail!("Unknown format: {}. Supported: sway, i3, instantwm", format),
        },
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

fn run_assist_selector(use_instantmenu: bool) -> Result<()> {
    let assists = registry::ASSISTS;

    if assists.is_empty() {
        println!("No assists available");
        return Ok(());
    }

    // If explicitly requested, use the instantmenu frontend.
    if use_instantmenu {
        return super::instantmenu::run_assist_selector_instantmenu();
    }

    // Default: use instantmenu server scratchpad + TUI chord navigator
    let client = client::MenuClient::new();
    client.show()?;
    client.ensure_server_running()?;

    // Build chord specifications from the tree structure
    let chord_specs = build_chord_specs(assists);

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
            format!("{}{}", mode_name, keys_hint)
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

fn export_instantwm_config(output_path: Option<std::path::PathBuf>) -> Result<()> {
    let default_path = dirs::config_dir()
        .map(|p| p.join("instantwm").join("assist.toml"))
        .context("Could not determine config directory")?;

    let (output_writer, path_used): (Box<dyn Write>, std::path::PathBuf) = match &output_path {
        Some(path) => (
            Box::new(File::create(path).context("Failed to create output file")?),
            path.clone(),
        ),
        None => {
            if let Some(parent) = default_path.parent() {
                std::fs::create_dir_all(parent)
                    .context("Failed to create instantWM config directory")?;
            }
            (
                Box::new(File::create(&default_path).context("Failed to create output file")?),
                default_path,
            )
        }
    };

    let mut output_writer = output_writer;

    writeln!(
        output_writer,
        "# instantWM config for instantCLI assists\n# Generated by `ins assist export --format instantwm`\n#\n# This file is automatically included when you run `ins assist export --format instantwm`.\n# To enter assist mode, add this keybind to your config.toml:\n#     [[keybinds]]\n#     modifiers = [\"Super\"]\n#     key = \"a\"\n#     action = {{ set_mode = \"instantassist\" }}\n"
    )?;

    fn generate_instantwm_modes<W: Write>(
        output: &mut W,
        entries: &[registry::AssistEntry],
        mode_name: &str,
        prefix: &str,
    ) -> Result<()> {
        writeln!(output, "[modes.{}]", mode_name)?;

        let description = if prefix.is_empty() {
            "instantassist mode".to_string()
        } else {
            let group = registry::find_group_entries(prefix)
                .and_then(|entries| entries.first())
                .map(|e| e.description());
            format!("{} submode", group.unwrap_or(mode_name))
        };
        writeln!(output, "description = \"{}\"", description)?;
        writeln!(output, "keybinds = [")?;

        writeln!(
            output,
            "  {{ modifiers = [], key = \"Escape\", action = {{ set_mode = \"default\" }} }},"
        )?;
        writeln!(
            output,
            "  {{ modifiers = [], key = \"Return\", action = {{ set_mode = \"default\" }} }},"
        )?;

        if !prefix.is_empty() {
            let help_cmd = format!("{}h", prefix);
            writeln!(
                output,
                "  {{ modifiers = [], key = \"h\", action = {{ spawn = [\"ins\", \"assist\", \"run\", \"{}\"] }} }},",
                help_cmd
            )?;
        }

        for entry in entries {
            match entry {
                registry::AssistEntry::Action(action) => {
                    if !prefix.is_empty() && action.key == 'h' {
                        continue;
                    }

                    let key_sequence = format!("{}{}", prefix, action.key);
                    writeln!(
                        output,
                        "  {{ modifiers = [], key = \"{}\", action = {{ spawn = [\"ins\", \"assist\", \"run\", \"{}\"] }} }},",
                        action.key, key_sequence
                    )?;
                }
                registry::AssistEntry::Group(group) => {
                    let sub_mode_name = format!("{}_{}", mode_name, group.key);
                    writeln!(
                        output,
                        "  {{ modifiers = [], key = \"{}\", action = {{ set_mode = \"{}\" }} }},",
                        group.key, sub_mode_name
                    )?;
                }
            }
        }

        writeln!(output, "]")?;
        writeln!(output)?;

        for entry in entries {
            if let registry::AssistEntry::Group(group) = entry {
                let sub_mode_name = format!("{}_{}", mode_name, group.key);
                let new_prefix = format!("{}{}", prefix, group.key);
                generate_instantwm_modes(output, group.children, &sub_mode_name, &new_prefix)?;
            }
        }

        Ok(())
    }

    generate_instantwm_modes(&mut output_writer, registry::ASSISTS, "instantassist", "")?;

    writeln!(output_writer, "# End of instantCLI assists config")?;

    drop(output_writer);

    println!("instantWM config written to: {}", path_used.display());

    if output_path.is_none()
        && let Some(config_dir) = dirs::config_dir()
    {
        let main_config = config_dir.join("instantwm").join("config.toml");
        if main_config.exists()
            && let Ok(content) = std::fs::read_to_string(&main_config)
            && !content.contains("assist.toml")
        {
            let include_entry = "\n[[includes]]\nfile = \"assist.toml\"\n";
            std::fs::OpenOptions::new()
                .append(true)
                .open(&main_config)
                .and_then(|mut f| std::io::Write::write_all(&mut f, include_entry.as_bytes()))
                .ok();
            println!("Added include to {}", main_config.display());
        }
    }

    let compositor = crate::common::compositor::CompositorType::detect();
    if compositor == crate::common::compositor::CompositorType::InstantWM {
        match crate::common::compositor::instantwm::reload_config() {
            Ok(()) => println!("instantWM config reloaded"),
            Err(e) => eprintln!("Warning: Failed to reload instantWM config: {}", e),
        }
    }

    Ok(())
}
