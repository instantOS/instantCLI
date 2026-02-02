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
    let wm = match command {
        SetupCommands::Sway => WindowManager::Sway,
        SetupCommands::I3 => WindowManager::I3,
    };
    setup_wm(wm)
}

fn setup_wm(wm: WindowManager) -> Result<()> {
    validate_compositor(&wm);
    let manager = WmConfigManager::new(wm);
    let config_changed = write_config_if_changed(&manager)?;
    let include_added = ensure_main_config_include(&manager, &wm)?;
    report_status(&wm, config_changed, include_added, &manager);
    if config_changed || include_added {
        maybe_reload_wm(&manager, &wm);
    }
    Ok(())
}

fn validate_compositor(wm: &WindowManager) {
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
}

fn write_config_if_changed(manager: &WmConfigManager) -> Result<bool> {
    let expected_content = generate_sway_config()?;
    let disk_hash = manager.hash_config().unwrap_or(0);
    let expected_hash = hash_string(&expected_content);
    let changed = disk_hash != expected_hash;
    if changed {
        manager.write_full_config(&expected_content)?;
    }
    Ok(changed)
}

fn ensure_main_config_include(manager: &WmConfigManager, wm: &WindowManager) -> Result<bool> {
    match manager.ensure_included_in_main_config() {
        Ok(added) => Ok(added),
        Err(e) => {
            emit(
                Level::Warn,
                &format!("setup.{}.include_failed", wm.name()),
                &format!(
                    "{} Could not add include to {} config: {}",
                    char::from(NerdFont::Warning),
                    wm.name(),
                    e
                ),
                None,
            );
            Ok(false)
        }
    }
}

fn report_status(
    wm: &WindowManager,
    config_changed: bool,
    include_added: bool,
    manager: &WmConfigManager,
) {
    if config_changed || include_added {
        emit(
            Level::Success,
            &format!("setup.{}.updated", wm.name()),
            &format!(
                "{} {} config updated",
                char::from(NerdFont::Check),
                wm.name()
            ),
            None,
        );
    } else {
        emit(
            Level::Info,
            &format!("setup.{}.unchanged", wm.name()),
            &format!(
                "{} {} config unchanged, skipping reload",
                char::from(NerdFont::Check),
                wm.name()
            ),
            None,
        );
    }
    emit(
        Level::Info,
        &format!("setup.{}.config_path", wm.name()),
        &format!("  Config file: {}", manager.config_path().display()),
        None,
    );
}

fn maybe_reload_wm(manager: &WmConfigManager, wm: &WindowManager) {
    match manager.reload() {
        Ok(()) => {
            emit(
                Level::Success,
                &format!("setup.{}.reloaded", wm.name()),
                &format!(
                    "{} {} configuration reloaded",
                    char::from(NerdFont::Sync),
                    wm.name()
                ),
                None,
            );
        }
        Err(e) => {
            emit(
                Level::Warn,
                &format!("setup.{}.reload_failed", wm.name()),
                &format!(
                    "{} Failed to reload {}: {}",
                    char::from(NerdFont::Warning),
                    wm.name(),
                    e
                ),
                None,
            );
        }
    }
}

/// Generate the full sway config content.
pub(crate) fn generate_sway_config() -> Result<String> {
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
    let keys_hint = build_keys_hint(entries, prefix);
    write_mode_header(output, mode_name, &keys_hint)?;
    write_help_binding(output, prefix)?;
    write_entry_bindings(output, entries, mode_name, prefix)?;
    writeln!(output, "}}\n")?;
    generate_submodes(output, entries, mode_name, prefix)?;

    Ok(())
}

/// Build a hint string showing available keys, filtering out 'h' in submodes.
fn build_keys_hint(entries: &[crate::assist::registry::AssistEntry], prefix: &str) -> String {
    use crate::assist::registry::AssistEntry;

    let mut keys: Vec<char> = entries
        .iter()
        .filter_map(|entry| {
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

    if keys.is_empty() {
        return String::new();
    }

    keys.sort_unstable();
    let keys_str: Vec<String> = keys.iter().map(|c| c.to_string()).collect();
    format!(" (keys: {})", keys_str.join(", "))
}

/// Write the mode header with name and exit bindings.
fn write_mode_header<W: Write>(output: &mut W, mode_name: &str, keys_hint: &str) -> Result<()> {
    let full_name = format!("{}{} (h for help)", mode_name, keys_hint);
    writeln!(output, "mode \"{}\" {{", full_name)?;
    writeln!(output, "    # Exit with Escape or Return")?;
    writeln!(output, "    bindsym Return mode default")?;
    writeln!(output, "    bindsym Escape mode default\n")?;
    Ok(())
}

/// Write the help binding for submodes (when prefix is not empty).
fn write_help_binding<W: Write>(output: &mut W, prefix: &str) -> Result<()> {
    if prefix.is_empty() {
        return Ok(());
    }
    let help_cmd = format!("ins assist run {}h", prefix);
    writeln!(
        output,
        "    # Show help for this mode\n    bindsym h exec --no-startup-id {}; mode default\n",
        help_cmd
    )?;
    Ok(())
}

/// Write action and group bindings for the current mode.
fn write_entry_bindings<W: Write>(
    output: &mut W,
    entries: &[crate::assist::registry::AssistEntry],
    mode_name: &str,
    prefix: &str,
) -> Result<()> {
    use crate::assist::registry::AssistEntry;

    for entry in entries {
        match entry {
            AssistEntry::Action(action) => {
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
                let sub_keys_hint = build_sub_keys_hint(&group.children);
                writeln!(
                    output,
                    "    bindsym {} mode \"{}_{}{} (h for help)\"",
                    group.key, mode_name, group.key, sub_keys_hint
                )?;
            }
        }
    }
    Ok(())
}

/// Build a keys hint for a group's children (excluding 'h').
fn build_sub_keys_hint(children: &[crate::assist::registry::AssistEntry]) -> String {
    use crate::assist::registry::AssistEntry;

    let mut keys: Vec<char> = children
        .iter()
        .filter_map(|child| match child {
            AssistEntry::Action(action) if action.key == 'h' => None,
            _ => Some(child.key()),
        })
        .collect();

    if keys.is_empty() {
        return String::new();
    }

    keys.sort_unstable();
    let keys_str: Vec<String> = keys.iter().map(|c| c.to_string()).collect();
    format!(" (keys: {})", keys_str.join(", "))
}

/// Recursively generate submodes for all groups.
fn generate_submodes<W: Write>(
    output: &mut W,
    entries: &[crate::assist::registry::AssistEntry],
    mode_name: &str,
    prefix: &str,
) -> Result<()> {
    use crate::assist::registry::AssistEntry;

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
