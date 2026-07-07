use crate::arch::config::DisplayManager;
use crate::arch::engine::{InstallContext, Question, QuestionId, QuestionResult};
use crate::menu_utils::{FzfPreview, FzfSelectable, FzfWrapper};
use crate::ui::catppuccin::colors;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;
use anyhow::Result;

#[derive(Clone)]
struct DisplayManagerOption(DisplayManager);

impl DisplayManagerOption {
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
        self.0.label().to_string()
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
impl Question for DisplayManagerQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::DisplayManager
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

    fn get_default(&self, _context: &InstallContext) -> Option<String> {
        Some(DisplayManager::DEFAULT.answer_value().to_string())
    }

    async fn ask(&self, _context: &InstallContext) -> Result<QuestionResult> {
        let options = vec![
            DisplayManagerOption(DisplayManager::Gdm),
            DisplayManagerOption(DisplayManager::Lightdm),
        ];

        let result = FzfWrapper::builder()
            .header(format!("{} Select Display Manager", NerdFont::Desktop))
            .select(options)?;

        match result {
            crate::menu_utils::FzfResult::Selected(option) => {
                Ok(QuestionResult::Answer(option.0.answer_value().to_string()))
            }
            _ => Ok(QuestionResult::Cancelled),
        }
    }

    fn validate(&self, _context: &InstallContext, answer: &str) -> Result<(), String> {
        match answer {
            "gdm" | "lightdm" => Ok(()),
            _ => Err("You must select a display manager.".to_string()),
        }
    }
}
