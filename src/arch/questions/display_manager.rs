use crate::arch::config::DisplayManager;
use crate::arch::engine::{InstallContext, StepId, StepOutcome, WizardStep};
use crate::menu_utils::{
    ConfirmResult, DialogOutcome, FzfPreview, FzfSelectable, FzfWrapper, HeaderBuilder,
};
use crate::ui::catppuccin::{colors, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;
use anyhow::Result;

#[derive(Clone)]
struct DisplayManagerOption(DisplayManager);

impl DisplayManagerOption {
    fn icon(&self) -> String {
        match self.0 {
            DisplayManager::Gdm => format_icon_colored(NerdFont::Desktop, colors::GREEN),
            DisplayManager::Lightdm => format_icon_colored(NerdFont::Desktop, colors::BLUE),
            DisplayManager::None => format_icon_colored(NerdFont::Terminal, colors::OVERLAY0),
        }
    }

    fn preview(&self) -> FzfPreview {
        match self.0 {
            DisplayManager::Gdm => PreviewBuilder::new()
                .header(NerdFont::Desktop, "gdm (recommended)")
                .subtext(
                    "The GNOME Display Manager. Highly reliable and supports Wayland natively.",
                )
                .blank()
                .line(colors::TEAL, None, "Best for")
                .bullets([
                    "Wayland-based setups (GNOME/Sway/Niri)",
                    "Clean, stable, modern look",
                ])
                .build(),
            DisplayManager::Lightdm => PreviewBuilder::new()
                .header(NerdFont::Desktop, "lightdm")
                .subtext("A lightweight, fast, and highly customizable display manager.")
                .blank()
                .line(colors::TEAL, None, "Best for")
                .bullets([
                    "Traditional GTK-based setups",
                    "Custom greeters and resource-constrained environments",
                ])
                .build(),
            DisplayManager::None => PreviewBuilder::new()
                .header(NerdFont::Terminal, "none (advanced)")
                .subtext("Install without a display manager. The system boots to a text console.")
                .blank()
                .line(colors::RED, Some(NerdFont::Warning), "Warning")
                .bullets([
                    "No graphical login screen will be shown",
                    "You must start your GUI session yourself after boot",
                ])
                .blank()
                .line(colors::TEAL, None, "Best for")
                .bullets([
                    "Minimal setups launched from the TTY (e.g. exec sway)",
                    "Users who prefer to manage their session manually",
                ])
                .build(),
        }
    }
}

impl FzfSelectable for DisplayManagerOption {
    fn fzf_display_text(&self) -> String {
        format!("{} {}", self.icon(), self.0.label())
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview()
    }

    fn fzf_key(&self) -> String {
        self.0.answer_value().to_string()
    }
}

pub struct DisplayManagerQuestion;

#[async_trait::async_trait]
impl WizardStep for DisplayManagerQuestion {
    fn id(&self) -> StepId {
        StepId::DisplayManager
    }

    fn description(&self) -> Option<&str> {
        Some("Choose the display manager (gdm, lightdm, or none)")
    }

    fn is_optional(&self) -> bool {
        true
    }

    fn should_ask(&self, context: &InstallContext) -> bool {
        crate::arch::config::DesktopEnvironment::from_context(context).requires_display_manager()
    }

    fn depends_on(&self) -> &[StepId] {
        &[StepId::DesktopEnvironment]
    }

    fn get_default(&self, _context: &InstallContext) -> Option<String> {
        Some(DisplayManager::DEFAULT.answer_value().to_string())
    }

    async fn run(&self, context: &InstallContext) -> Result<StepOutcome> {
        loop {
            let options = vec![
                DisplayManagerOption(DisplayManager::Gdm),
                DisplayManagerOption(DisplayManager::Lightdm),
                DisplayManagerOption(DisplayManager::None),
            ];

            let result = super::select_one_for_step(
                context,
                self,
                FzfWrapper::builder()
                    .header(HeaderBuilder::new(NerdFont::Desktop, "Select Display Manager").build())
                    .items(options)
                    .padded(),
            )?;

            let option = match result {
                DialogOutcome::Submitted(option) => option,
                DialogOutcome::Cancelled => return Ok(StepOutcome::Pause),
            };

            if option.0 != DisplayManager::None {
                return Ok(StepOutcome::Answer(option.0.answer_value().to_string()));
            }

            // Warn that choosing no display manager means booting to a
            // text console and starting the GUI manually.
            let confirmed = FzfWrapper::builder()
                .confirm(format!(
                    "{} No display manager selected\n\n\
                     The system will boot to a text console.\n\
                     You will need to start your GUI session yourself after login.",
                    NerdFont::Warning
                ))
                .confirm_dialog()?;

            match confirmed {
                ConfirmResult::Yes => {
                    return Ok(StepOutcome::Answer(option.0.answer_value().to_string()));
                }
                // Go back to the selection so the user can reconsider.
                ConfirmResult::No | ConfirmResult::Cancelled => continue,
            }
        }
    }

    fn validate(&self, _context: &InstallContext, answer: &str) -> Result<(), String> {
        match answer {
            "gdm" | "lightdm" | "none" => Ok(()),
            _ => Err("You must select a display manager.".to_string()),
        }
    }
}
