//! Setup command implementations
//!
//! Handles the `ins setup` command and its subcommands.

use anyhow::{Context, Result};
use clap::Subcommand;
use std::io::Write;

use crate::common::compositor::CompositorType;
use crate::common::compositor::config::{WindowManager, WmConfigManager};
use crate::ui::prelude::*;

#[derive(Subcommand, Debug, Clone)]
pub enum SetupCommands {
    /// Set up Sway window manager integration
    ///
    /// This command:
    /// - Exports assist keybinds to the shared config file
    /// - Configures cursor theme
    /// - Adds an include to your main sway config
    /// - Reloads Sway to apply changes
    Sway,

    /// Set up i3 window manager integration
    ///
    /// This command:
    /// - Exports assist keybinds to the shared config file
    /// - Adds an include to your main i3 config
    /// - Reloads i3 to apply changes
    I3,
}

/// Handle setup command dispatch
pub fn handle_setup_command(command: SetupCommands) -> Result<()> {
    match command {
        SetupCommands::Sway => setup_sway(),
        SetupCommands::I3 => setup_i3(),
    }
}

/// Set up Sway window manager integration
pub fn setup_sway() -> Result<()> {
    setup_wm(WindowManager::Sway)
}

/// Set up i3 window manager integration
pub fn setup_i3() -> Result<()> {
    setup_wm(WindowManager::I3)
}

/// Generic window manager setup
fn setup_wm(wm: WindowManager) -> Result<()> {
    let compositor = CompositorType::detect();
    let expected_compositor = match wm {
        WindowManager::Sway => CompositorType::Sway,
        WindowManager::I3 => CompositorType::I3,
    };

    if compositor != expected_compositor {
        emit(
            Level::Warn,
            &format!("setup.{}.wrong_compositor", wm.name()),
            &format!(
                "{} Current compositor is {}, not {}. Setup will proceed but may not work correctly.",
                char::from(NerdFont::Warning),
                compositor.name(),
                wm.name()
            ),
            None,
        );
    }

    let manager = WmConfigManager::new(wm);

    // Generate the full expected config content
    let expected_content = generate_sway_config()?;

    // Compare hash of expected content with what's on disk
    let disk_hash = manager.hash_config().unwrap_or(0);
    let expected_hash = hash_string(&expected_content);
    let config_changed = disk_hash != expected_hash;

    // Write if changed
    if config_changed {
        manager.write_full_config(&expected_content)?;
    }

    // Ensure include exists in main config
    let include_added = match manager.ensure_included_in_main_config() {
        Ok(added) => added,
        Err(e) => {
            emit(
                Level::Warn,
                "setup.sway.include_failed",
                &format!(
                    "{} Could not add include to sway config: {}",
                    char::from(NerdFont::Warning),
                    e
                ),
                None,
            );
            false
        }
    };

    if config_changed || include_added {
        emit(
            Level::Success,
            "setup.sway.updated",
            &format!("{} Sway config updated", char::from(NerdFont::Check)),
            None,
        );
        emit(
            Level::Info,
            "setup.sway.config_path",
            &format!("  Config file: {}", manager.config_path().display()),
            None,
        );

        // Reload sway
        match manager.reload() {
            Ok(()) => {
                emit(
                    Level::Success,
                    "setup.sway.reloaded",
                    &format!("{} Sway configuration reloaded", char::from(NerdFont::Sync)),
                    None,
                );
            }
            Err(e) => {
                emit(
                    Level::Warn,
                    "setup.sway.reload_failed",
                    &format!(
                        "{} Failed to reload Sway: {}",
                        char::from(NerdFont::Warning),
                        e
                    ),
                    None,
                );
            }
        }
    } else {
        emit(
            Level::Info,
            "setup.sway.unchanged",
            &format!(
                "{} Sway config unchanged, skipping reload",
                char::from(NerdFont::Check)
            ),
            None,
        );
        emit(
            Level::Info,
            "setup.sway.config_path",
            &format!("  Config file: {}", manager.config_path().display()),
            None,
        );
    }

    Ok(())
}

/// Generate the full sway config content.
fn generate_sway_config() -> Result<String> {
    use std::fmt::Write;

    let mut content = String::new();

    // Header
    writeln!(content, "# instantCLI sway configuration")?;
    writeln!(
        content,
        "# This file is managed by instantCLI. Manual edits may be overwritten."
    )?;
    writeln!(content)?;

    // Cursor theme section
    if let Ok(theme) = get_current_cursor_theme()
        && !theme.is_empty()
    {
        writeln!(content, "# --- BEGIN cursor_theme ---")?;
        writeln!(content, "seat * xcursor_theme {}", theme)?;
        writeln!(content, "# --- END cursor_theme ---")?;
        writeln!(content)?;
    }

    // Assist keybinds section
    writeln!(content, "# --- BEGIN assist ---")?;
    let keybinds = export_assist_keybinds()?;
    write!(content, "{}", keybinds.trim())?;
    writeln!(content)?;
    writeln!(content, "# --- END assist ---")?;

    Ok(content)
}

/// Hash a string for comparison.
fn hash_string(s: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

/// Export assist keybinds to a string for inclusion in sway config.
///
/// This generates the same output as `ins assist export --format sway` but
/// returns it as a string instead of writing to a file.
fn export_assist_keybinds() -> Result<String> {
    use crate::assist::registry;

    let mut output = Vec::new();

    // Write header
    writeln!(
        output,
        "# Sway keybinds for instantCLI assists\n# Generated by `ins setup sway`\n"
    )?;

    // Generate mode for instantassist
    writeln!(output, "# Enter instantassist mode")?;

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
        output,
        "bindsym $mod+a mode \"instantassist{} (h for help)\"\n",
        root_keys_hint
    )?;

    // Generate all modes recursively
    generate_modes(&mut output, registry::ASSISTS, "instantassist", "")?;

    writeln!(output, "# End of instantCLI assists config")?;

    Ok(String::from_utf8(output)?)
}

/// Helper function to generate modes recursively for groups
fn generate_modes<W: Write>(
    output: &mut W,
    entries: &[crate::assist::registry::AssistEntry],
    mode_name: &str,
    prefix: &str,
) -> Result<()> {
    use crate::assist::registry::AssistEntry;

    let mut available_keys: Vec<char> = entries
        .iter()
        .filter_map(|entry| {
            // Filter out 'h' if it's already handled as a help binding in a submode
            if !prefix.is_empty() {
                match entry {
                    AssistEntry::Action(action) if action.key == 'h' => None,
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

    let mode_name_with_hint = format!("{}{} (h for help)", mode_name, keys_hint);
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
            AssistEntry::Action(action) => {
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
            AssistEntry::Group(group) => {
                // Collect available keys for the submode
                let mut sub_keys: Vec<char> = group
                    .children
                    .iter()
                    .filter_map(|child| match child {
                        AssistEntry::Action(action) if action.key == 'h' => None,
                        _ => Some(child.key()),
                    })
                    .collect();
                sub_keys.sort_unstable();
                let sub_keys_hint = if sub_keys.is_empty() {
                    "".to_string()
                } else {
                    let keys_str: Vec<String> = sub_keys.iter().map(|c| c.to_string()).collect();
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
        if let AssistEntry::Group(group) = entry {
            let sub_mode_name = format!("{}_{}", mode_name, group.key);
            let new_prefix = format!("{}{}", prefix, group.key);
            generate_modes(output, group.children, &sub_mode_name, &new_prefix)?;
        }
    }

    Ok(())
}

/// Get the current cursor theme from gsettings.
fn get_current_cursor_theme() -> Result<String> {
    let output = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "cursor-theme"])
        .output()
        .context("Failed to query cursor theme from gsettings")?;

    let theme = String::from_utf8_lossy(&output.stdout);
    // Remove quotes and whitespace
    Ok(theme
        .trim()
        .trim_matches('\'')
        .trim_matches('"')
        .to_string())
}
