use std::fs;

use anyhow::{Context, Result, bail};

use crate::common::systemd::SystemdManager;
use crate::menu_utils::{
    ConfirmResult, FzfWrapper, TextEditOutcome, TextEditPrompt, prompt_text_edit,
};
use crate::settings::context::SettingsContext;

use super::COMBINE_SINK_CONFIG_FILE;
use super::config::{
    combine_sink_config_file, config_changed, display_name_to_node_name, get_current_config,
    get_current_sink_name, is_combined_sink_enabled, pipewire_config_path,
};
use super::wpctl::set_combined_sink_as_default;

/// Remove the combined sink by deleting the config file
/// Returns true if a restart is needed (config file existed and was removed)
pub(super) fn remove_combined_sink(ctx: &SettingsContext) -> Result<bool> {
    let config_path = combine_sink_config_file()?;

    // Only restart if there was actually a config to remove
    let needs_restart = config_path.exists();

    if needs_restart {
        fs::remove_file(&config_path)
            .with_context(|| format!("Failed to remove {:?}", config_path))?;
        ctx.notify("Combined Audio Sink", "Combined sink removed.");
    } else {
        ctx.notify("Combined Audio Sink", "Combined sink was not configured.");
    }

    Ok(needs_restart)
}

/// Enable and configure the combined sink
/// Returns true if a restart is needed (config changed), false otherwise
pub(super) fn enable_combined_sink(
    ctx: &SettingsContext,
    selected_node_names: &[String],
    display_name: &str,
) -> Result<bool> {
    if selected_node_names.len() < 2 {
        bail!("Select at least 2 devices to combine");
    }

    // Check if anything actually changed
    let needs_restart = config_changed(selected_node_names, display_name)?;

    // Skip writing if nothing changed
    if !needs_restart {
        return Ok(false);
    }

    // Generate the node.name with our prefix for detection
    let node_name = display_name_to_node_name(display_name);

    // Build the matches array for the config
    let matches: Vec<String> = selected_node_names
        .iter()
        .map(|name| format!("                    {{ node.name = \"{}\" }}", name))
        .collect();

    // Generate the PipeWire config with prefixed node.name for detection
    let config = format!(
        r#"context.modules = [
 {{   name = libpipewire-module-combine-stream
     args = {{
         combine.mode = sink
         node.name = "{}"
         node.description = "{}"
         combine.props = {{
             audio.position = [ FL FR ]
         }}
         stream.rules = [
             {{
                 matches = [
 {}
                 ]
                 actions = {{
                     create-stream = {{ }}
                 }}
             }}
         ]
     }}
 }}
 ]
 "#,
        node_name,
        display_name,
        matches.join("\n")
    );

    // Ensure the config directory exists
    let config_dir = pipewire_config_path()?;
    fs::create_dir_all(&config_dir)
        .with_context(|| format!("Failed to create directory {:?}", config_dir))?;

    // Write the config file
    let config_path = config_dir.join(COMBINE_SINK_CONFIG_FILE);
    fs::write(&config_path, config)
        .with_context(|| format!("Failed to write config to {:?}", config_path))?;

    ctx.notify(
        "Combined Audio Sink",
        &format!(
            "Combined sink '{}' configured with {} devices. Restart required to activate.",
            display_name,
            selected_node_names.len()
        ),
    );

    Ok(true)
}

/// Restart PipeWire services to apply configuration changes
fn restart_pipewire_services(ctx: &SettingsContext) -> Result<()> {
    let manager = SystemdManager::user();

    ctx.emit_info(
        "audio.combined_sink.restarting",
        "Restarting PipeWire services...",
    );

    // Restart the main PipeWire services in order
    // wireplumber should auto-restart since it depends on pipewire
    manager.restart("pipewire")?;

    ctx.emit_success(
        "audio.combined_sink.restarted",
        "PipeWire services restarted successfully.",
    );

    Ok(())
}

/// Offer to restart PipeWire services after configuration change
pub(super) fn offer_restart(ctx: &SettingsContext) -> Result<()> {
    let result = FzfWrapper::builder()
        .confirm("PipeWire needs to be restarted for changes to take effect.\n\nAudio will be briefly interrupted during restart.")
        .yes_text("Restart PipeWire")
        .no_text("Restart Later (manual)")
        .confirm_dialog()?;

    match result {
        ConfirmResult::Yes => {
            if let Err(e) = restart_pipewire_services(ctx) {
                ctx.emit_failure(
                    "audio.combined_sink.restart_failed",
                    &format!("Failed to restart PipeWire: {}", e),
                );
                ctx.show_message(&format!(
                    "Failed to restart PipeWire: {}\n\nPlease restart manually:\n  systemctl --user restart pipewire",
                    e
                ));
            }
        }
        ConfirmResult::No | ConfirmResult::Cancelled => {
            ctx.emit_info(
                "audio.combined_sink.restart_skipped",
                "PipeWire restart skipped. Run 'systemctl --user restart pipewire' to apply changes.",
            );
        }
    }

    Ok(())
}

/// Rename the combined sink
/// Returns true if a restart is needed (sink was enabled and name changed)
pub(super) fn rename_combined_sink(ctx: &SettingsContext) -> Result<bool> {
    let current_name = get_current_sink_name();

    let result = prompt_text_edit(
        TextEditPrompt::new("Combined sink name", None)
            .header("Rename your combined audio sink")
            .ghost(&current_name),
    )?;

    let new_name = match result {
        TextEditOutcome::Updated(Some(name)) => name,
        // Empty input with ghost text showing current name = keep current name
        TextEditOutcome::Updated(None) => current_name.clone(),
        TextEditOutcome::Cancelled | TextEditOutcome::Unchanged => return Ok(false),
    };

    // Don't update if name is the same
    if new_name == current_name {
        return Ok(false);
    }

    // If combined sink is enabled, we need to regenerate the config with the new name
    let needs_restart = if is_combined_sink_enabled() {
        let (stored_devices, _) = get_current_config();
        if stored_devices.len() >= 2 {
            let node_names: Vec<String> = stored_devices.into_iter().collect();
            // This will write the new config and return if restart is needed
            enable_combined_sink(ctx, &node_names, &new_name)?
        } else {
            false
        }
    } else {
        false
    };

    ctx.notify("Combined Audio Sink", &format!("Renamed to '{}'", new_name));

    Ok(needs_restart)
}

/// Offer to set the combined sink as the default output after creation
pub(super) fn offer_set_as_default(ctx: &SettingsContext) -> Result<()> {
    let result = FzfWrapper::builder()
        .confirm("Would you like to set the combined sink as your default audio output?")
        .yes_text("Set as default")
        .no_text("Keep current default")
        .confirm_dialog()?;

    match result {
        ConfirmResult::Yes => {
            if let Err(e) = set_combined_sink_as_default(ctx) {
                ctx.emit_failure(
                    "audio.combined_sink.set_default_failed",
                    &format!("Failed to set as default: {}", e),
                );
            }
        }
        ConfirmResult::No | ConfirmResult::Cancelled => {}
    }

    Ok(())
}
