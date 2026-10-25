use anyhow::Result;

use crate::arch::engine::{InstallContext, StepId, StepOutcome, WizardStep};
use crate::common::console_font::{ConsoleFont, DEFAULT_NAME};
use crate::common::console_font_picker::{FontSession, pick_font};
use crate::menu_utils::DialogOutcome;

pub struct ConsoleFontQuestion;

#[async_trait::async_trait]
impl WizardStep for ConsoleFontQuestion {
    fn id(&self) -> StepId {
        StepId::ConsoleFont
    }
    fn description(&self) -> Option<&str> {
        Some("Choose the font size for Linux TTYs")
    }
    fn preselect_answer(&self, _context: &InstallContext) -> Option<String> {
        Some(DEFAULT_NAME.to_string())
    }
    fn validate(&self, _context: &InstallContext, answer: &str) -> Result<(), String> {
        ConsoleFont::parse(answer)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    async fn run(&self, context: &InstallContext) -> Result<StepOutcome> {
        let mut session = crate::ui::catppuccin::linux_console_device()
            .map(|console| FontSession::start(console.to_path_buf(), false))
            .transpose()
            .unwrap_or_else(|error| {
                eprintln!("Warning: live font preview is unavailable: {error:#}");
                None
            });
        let result = match pick_font(
            context.previous_answer(&self.id()).map(String::as_str),
            session.as_ref(),
        ) {
            Ok(result) => result,
            Err(error) => {
                return Ok(StepOutcome::Retry(format!(
                    "Could not select console font: {error:#}"
                )));
            }
        };
        match result {
            DialogOutcome::Submitted(option) => {
                let answer = option.to_answer()?;
                if let Some(session) = session.as_mut() {
                    session.confirm();
                }
                Ok(StepOutcome::Answer(answer))
            }
            DialogOutcome::Cancelled => Ok(StepOutcome::Pause),
        }
    }
}
