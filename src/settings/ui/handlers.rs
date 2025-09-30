use anyhow::{Context, Result};
use duct::cmd;
use std::process::Command;

use crate::settings::registry::{SettingDefinition, SettingKind};

use super::super::context::{
    ApplyOverride, SettingsContext, apply_definition, select_one_with_style,
};
use super::super::registry::CommandStyle;
use super::items::{ChoiceItem, SettingState};

pub fn handle_setting(
    ctx: &mut SettingsContext,
    definition: &'static SettingDefinition,
    state: SettingState,
) -> Result<()> {
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
            ctx.emit_info("settings.action.running", &summary.to_string());
            ctx.with_definition(definition, run)?;
        }
        SettingKind::Command {
            summary,
            command,
            required,
        } => {
            let mut missing = Vec::new();
            for pkg in *required {
                let installed = pkg.ensure()?;
                if !installed {
                    missing.push(pkg);
                }
            }

            if !missing.is_empty() {
                for pkg in missing {
                    ctx.emit_info(
                        "settings.command.missing",
                        &format!(
                            "{} missing dependency `{}`. {}",
                            definition.title,
                            pkg.name,
                            pkg.install_hint()
                        ),
                    );
                }
                return Ok(());
            }

            ctx.emit_info("settings.command.launching", &summary.to_string());

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
