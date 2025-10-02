use anyhow::{Context, Result};
use duct::cmd;
use std::process::Command;

use crate::fzf_wrapper::FzfWrapper;
use crate::settings::registry::{SettingDefinition, SettingKind, SettingRequirement};

use super::super::context::{
    ApplyOverride, SettingsContext, apply_definition, select_one_with_style,
};
use super::super::registry::CommandStyle;
use super::items::{ChoiceItem, SettingState};

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
            let items: Vec<ChoiceItem> = options
                .iter()
                .enumerate()
                .map(|(index, option)| ChoiceItem {
                    option,
                    is_current: matches!(
                        state,
                        SettingState::Choice {
                            current_index: Some(current)
                        } if current == index
                    ),
                    summary,
                })
                .collect();

            match select_one_with_style(items)? {
                Some(choice) => {
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
                }
                None => ctx.emit_info("settings.choice.cancelled", "No changes made."),
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
