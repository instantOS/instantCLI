use anyhow::{Context, Result};
use duct::cmd;
use std::process::Command;

use crate::menu_utils::FzfWrapper;
use crate::settings::registry::{SettingDefinition, SettingKind, SettingRequirement};

use super::super::context::{
    ApplyOverride, SettingsContext, apply_definition, select_one_with_style_at,
};
use super::super::registry::CommandStyle;
use super::items::{ChoiceItem, ChoiceMenuItem, SettingState};

/// Find the index of a choice menu item in the list
fn choice_menu_index(items: &[ChoiceMenuItem], selected: ChoiceMenuItem) -> Option<usize> {
    items
        .iter()
        .enumerate()
        .find_map(|(idx, item)| match (item, selected) {
            (ChoiceMenuItem::Back, ChoiceMenuItem::Back) => Some(idx),
            (ChoiceMenuItem::Option(lhs), ChoiceMenuItem::Option(rhs))
                if lhs.option.value == rhs.option.value =>
            {
                Some(idx)
            }
            _ => None,
        })
}

/// Check and handle requirements for a setting
fn ensure_requirements(
    ctx: &mut SettingsContext,
    definition: &'static SettingDefinition,
) -> Result<bool> {
    let mut unmet = Vec::new();

    for requirement in definition.requirements {
        match requirement {
            SettingRequirement::Package(pkg) => {
                if !pkg.ensure()? {
                    unmet.push(requirement);
                }
            }
            SettingRequirement::Condition { check, .. } => {
                if !check() {
                    unmet.push(requirement);
                }
            }
        }
    }

    if !unmet.is_empty() {
        let mut messages = Vec::new();
        messages.push(format!(
            "Cannot use '{}' - requirements not met:",
            definition.title
        ));
        messages.push(String::new());
        for req in &unmet {
            messages.push(format!("  • {}", req.description()));
            messages.push(format!("    {}", req.resolve_hint()));
            messages.push(String::new());
        }

        FzfWrapper::builder()
            .message(messages.join("\n"))
            .title("Requirements Not Met")
            .show_message()?;

        return Ok(false);
    }

    Ok(true)
}

pub fn handle_setting(
    ctx: &mut SettingsContext,
    definition: &'static SettingDefinition,
    state: SettingState,
) -> Result<()> {
    // Check requirements before allowing any action
    if !definition.requirements.is_empty() && !ensure_requirements(ctx, definition)? {
        return Ok(());
    }

    match &definition.kind {
        SettingKind::Toggle { key, apply, .. } => {
            let current = matches!(state, SettingState::Toggle { enabled: true });
            let target = !current;

            ctx.set_bool(*key, target);
            if apply.is_some() {
                apply_definition(ctx, definition, Some(ApplyOverride::Bool(target)))?;
            }

            ctx.emit_success(
                "settings.toggle.updated",
                &format!(
                    "{} {}",
                    definition.title,
                    if target { "enabled" } else { "disabled" }
                ),
            );
        }
        SettingKind::Choice {
            key,
            options,
            summary,
            apply,
        } => {
            // Loop to allow user to try different values and see the changes
            let mut cursor = None;

            loop {
                // Get current value to mark it in the menu
                let current_value = ctx.string(*key);
                let current_index = options.iter().position(|opt| opt.value == current_value);

                // Build menu items with current selection marked
                let mut items: Vec<ChoiceMenuItem> = options
                    .iter()
                    .enumerate()
                    .map(|(index, option)| {
                        ChoiceMenuItem::Option(ChoiceItem {
                            option,
                            is_current: current_index == Some(index),
                            summary,
                        })
                    })
                    .collect();

                // Add Back option
                items.push(ChoiceMenuItem::Back);

                match select_one_with_style_at(items.clone(), cursor)? {
                    Some(selected) => {
                        // Update cursor to maintain position
                        if let Some(index) = choice_menu_index(&items, selected) {
                            cursor = Some(index);
                        }

                        match selected {
                            ChoiceMenuItem::Option(choice) => {
                                ctx.set_string(*key, choice.option.value);
                                if apply.is_some() {
                                    apply_definition(
                                        ctx,
                                        definition,
                                        Some(ApplyOverride::Choice(choice.option)),
                                    )?;
                                }
                                ctx.emit_success(
                                    "settings.choice.updated",
                                    &format!("{} set to {}", definition.title, choice.option.label),
                                );
                                // Continue loop to show updated menu
                            }
                            ChoiceMenuItem::Back => {
                                // User selected Back, exit the loop
                                break;
                            }
                        }
                    }
                    None => {
                        // User cancelled, exit the loop
                        break;
                    }
                }
            }
        }
        SettingKind::Action { summary, run } => {
            ctx.emit_info("settings.action.running", summary.as_ref());
            ctx.with_definition(definition, run)?;
        }
        SettingKind::Command { summary, command } => {
            ctx.emit_info("settings.command.launching", summary.as_ref());

            ctx.with_definition(definition, |ctx| {
                match command.style {
                    CommandStyle::Terminal => {
                        cmd(command.program, command.args)
                            .run()
                            .with_context(|| format!("running {}", command.program))?;
                    }
                    CommandStyle::Detached => {
                        Command::new(command.program)
                            .args(command.args)
                            .spawn()
                            .with_context(|| format!("spawning {}", command.program))?;
                    }
                }
                Ok(())
            })?;

            ctx.emit_success(
                "settings.command.completed",
                &format!("Launched {}", definition.title),
            );
        }
    }

    Ok(())
}
