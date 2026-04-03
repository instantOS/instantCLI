use anyhow::{Result, anyhow};

use crate::game::launch_command::{LaunchCommand, LaunchCommandKind, SteamLaunchCommand};
use crate::menu_utils::{TextEditPrompt, prompt_text_edit};

use super::prompts::confirm_command;

pub struct SteamBuilder;

impl SteamBuilder {
    pub fn build_command(app_id_hint: Option<u32>) -> Result<Option<LaunchCommand>> {
        let app_id = match Self::select_app_id(app_id_hint)? {
            Some(app_id) => app_id,
            None => return Ok(None),
        };

        let command = LaunchCommand {
            wrappers: Default::default(),
            kind: LaunchCommandKind::Steam(SteamLaunchCommand { app_id }),
        };

        if confirm_command(&command)? {
            Ok(Some(command))
        } else {
            Ok(None)
        }
    }

    fn select_app_id(app_id_hint: Option<u32>) -> Result<Option<u32>> {
        let initial = app_id_hint.map(|value| value.to_string());
        let header = app_id_hint
            .map(|value| format!("Enter Steam App ID\nDetected from Proton prefix: {value}"))
            .unwrap_or_else(|| "Enter Steam App ID".to_string());

        match prompt_text_edit(
            TextEditPrompt::new("Steam App ID", initial.as_deref())
                .header(header)
                .ghost("Example: 1245620"),
        )? {
            crate::menu_utils::TextEditOutcome::Updated(Some(raw)) => {
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    return Ok(None);
                }
                let app_id = trimmed
                    .parse::<u32>()
                    .map_err(|_| anyhow!("Steam App ID must be a positive integer"))?;
                Ok(Some(app_id))
            }
            crate::menu_utils::TextEditOutcome::Unchanged => Ok(app_id_hint),
            crate::menu_utils::TextEditOutcome::Cancelled => Ok(None),
            crate::menu_utils::TextEditOutcome::Updated(None) => Ok(None),
        }
    }
}
