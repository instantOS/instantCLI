use crate::arch::engine::{InstallContext, StepId, StepOutcome, WizardStep};
use crate::menu_utils::FzfWrapper;
use crate::ui::nerd_font::NerdFont;
use anyhow::Result;

type AnswerValidator = dyn Fn(&str) -> Result<(), String> + Send + Sync;

/// Reusable free-text input question.
///
/// Mirrors [`BooleanQuestion`]'s builder style: shared ask and validation
/// behavior, with the prompt and validation rules supplied at construction.
/// See [`validators`] for the common rules.
pub struct TextInputQuestion {
    id: StepId,
    prompt: String,
    icon: NerdFont,
    description: Option<String>,
    validators: Vec<Box<AnswerValidator>>,
}

impl TextInputQuestion {
    pub fn new(id: StepId, prompt: impl Into<String>, icon: NerdFont) -> Self {
        Self {
            id,
            prompt: prompt.into(),
            icon,
            description: None,
            validators: Vec::new(),
        }
    }

    /// Short human-readable description shown in the review menu preview.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Register a validation rule for candidate answers.
    ///
    /// Rules run in registration order; the first failure is reported.
    pub fn validator<F>(mut self, validator: F) -> Self
    where
        F: Fn(&str) -> Result<(), String> + Send + Sync + 'static,
    {
        self.validators.push(Box::new(validator));
        self
    }
}

#[async_trait::async_trait]
impl WizardStep for TextInputQuestion {
    fn id(&self) -> StepId {
        self.id
    }

    fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    async fn run(&self, _context: &InstallContext) -> Result<StepOutcome> {
        let result = FzfWrapper::builder()
            .prompt(format!("{} {}", self.icon, self.prompt))
            .input()
            .input_result()?;

        Ok(StepOutcome::from_selection(result, |answer| answer))
    }

    fn validate(&self, _context: &InstallContext, answer: &str) -> Result<(), String> {
        for validator in &self.validators {
            validator(answer)?;
        }
        Ok(())
    }
}

/// Validation rules for installer-provided names.
pub mod validators {
    /// Reject a specific reserved value (e.g. the `root` username).
    pub fn forbidden_value(
        label: &str,
        forbidden: &str,
    ) -> impl Fn(&str) -> Result<(), String> + Send + Sync {
        let label = label.to_string();
        move |answer| {
            if answer == forbidden {
                return Err(format!("{label} cannot be '{forbidden}'."));
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validators_run_in_order_and_report_the_first_failure() {
        let question = TextInputQuestion::new(StepId::Username, "prompt", NerdFont::User)
            .validator(validators::forbidden_value("Username", "root"))
            .validator(validators::forbidden_value("Username", "admin"));

        assert_eq!(
            question.validate(&InstallContext::new(), "ben").map(|_| ()),
            Ok(())
        );
        assert_eq!(
            question
                .validate(&InstallContext::new(), "root")
                .map(|_| ()),
            Err("Username cannot be 'root'.".to_string())
        );
        assert_eq!(
            question
                .validate(&InstallContext::new(), "admin")
                .map(|_| ()),
            Err("Username cannot be 'admin'.".to_string())
        );
    }
}
