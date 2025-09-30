use anyhow::Result;

use crate::settings::registry::{SettingDefinition, SettingKind};

use super::super::context::{apply_definition, format_icon, select_one_with_style, SettingsContext};
use super::super::registry::CommandStyle;
use super::items::{ChoiceItem, SettingState, ToggleChoiceItem};

pub fn handle_setting(
    ctx: &mut SettingsContext,
    definition: &'static SettingDefinition,
    state: SettingState,
) -> Result<()> {
    match &definition.kind {
        SettingKind::Toggle {
            key,
            summary,
            apply,
        } => {
            let current = matches!(state, SettingState::Toggle { enabled: true });
            let choices = vec![
                ToggleChoiceItem {
                    title: definition.title,
                    summary,
                    target_enabled: true,
                    current_enabled: current,
                },
                ToggleChoiceItem {
                    title: definition.title,
                    summary,
                    target_enabled: false,
                    current_enabled: current,
                },
            ];

            match select_one_with_style(choices)? {
                Some(choice) => {
                    if choice.target_enabled == current {
                        ctx.emit_info(
                            "settings.toggle.noop",
                            &format!(
                                "{} is already {}.",
                                definition.title,
                                if current { "enabled" } else { "disabled" }
                            ),
                        );
                        return Ok(());
                    }

                    ctx.set_bool(*key, choice.target_enabled);
                    if apply.is_some() {
                        apply_definition(
                            ctx,
                            definition,
                            Some(super::super::context::ApplyOverride::Bool(
                                choice.target_enabled,
                            )),
                        )?;
                    }
                    ctx.emit_success(
                        "settings.toggle.updated",
                        &format!(
                            "{} {}",
                            definition.title,
                            if choice.target_enabled {
                                "enabled"
                            } else {
                                "disabled"
                            }
                        ),
                    );
                }
                None => ctx.emit_info("settings.toggle.cancelled", "No changes made."),
            }
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
                            Some(super::super::context::ApplyOverride::Choice(
                                choice.option,
                            )),
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
                        duct::cmd(command.program, command.args)
                            .run()
                            .with_context(|| format!("running {}", command.program))?;
                    }
                    CommandStyle::Detached => {
                        std::process::Command::new(command.program)
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
