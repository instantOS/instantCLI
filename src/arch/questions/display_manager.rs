use crate::arch::config::DisplayManager;
use crate::arch::engine::{InstallContext, StepId, StepOutcome, WizardStep};
use crate::menu_utils::{FzfPreview, FzfSelectable, FzfWrapper, HeaderBuilder, MenuPresentation};
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
        Some("Choose the display manager (gdm or lightdm)")
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

    async fn run(&self, _context: &InstallContext) -> Result<StepOutcome> {
        let options = vec![
            DisplayManagerOption(DisplayManager::Gdm),
            DisplayManagerOption(DisplayManager::Lightdm),
        ];

        let result = FzfWrapper::builder()
            .header(HeaderBuilder::new(NerdFont::Desktop, "Select Display Manager").build())
            .presentation(MenuPresentation::Padded)
            .select_one(options)?;

        Ok(StepOutcome::from_dialog(result, |option| {
            option.0.answer_value().to_string()
        }))
    }

    fn validate(&self, _context: &InstallContext, answer: &str) -> Result<(), String> {
        match answer {
            "gdm" | "lightdm" => Ok(()),
            _ => Err("You must select a display manager.".to_string()),
        }
    }
}
