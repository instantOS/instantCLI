use crate::arch::engine::{AskPolicy, InstallContext, StepId, StepOutcome, WizardStep};
use crate::menu_utils::{ConfirmResult, FzfWrapper};
use crate::ui::nerd_font::NerdFont;
use anyhow::Result;

type ContextPredicate = dyn Fn(&InstallContext) -> bool + Send + Sync;

/// The unattended answer of an optional boolean question. Booleans always
/// have one, so optionality and the answer applied unattended coincide:
/// `None` on the question means required, `Some` means optional.
enum BooleanDefault {
    No,
    Yes,
    Derived(Box<ContextPredicate>),
}

impl BooleanDefault {
    /// The answer recorded when the question runs unattended, using the
    /// same machine values `run` records for the same choice.
    fn unattended_answer(&self, context: &InstallContext) -> &'static str {
        match self {
            Self::No => "no",
            Self::Yes => "yes",
            Self::Derived(predicate) => {
                if predicate(context) {
                    "yes"
                } else {
                    "no"
                }
            }
        }
    }
}

pub struct BooleanQuestion {
    id: StepId,
    prompt: String,
    description: Option<String>,
    icon: NerdFont,
    default: Option<BooleanDefault>,
    should_ask_predicate: Option<Box<ContextPredicate>>,
    dependencies: Vec<StepId>,
}

impl BooleanQuestion {
    pub fn new(id: StepId, prompt: impl Into<String>, icon: NerdFont) -> Self {
        Self {
            id,
            prompt: prompt.into(),
            description: None,
            icon,
            default: None,
            should_ask_predicate: None,
            dependencies: Vec::new(),
        }
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Make the question optional; its unattended answer is "no".
    pub fn optional(mut self) -> Self {
        self.default = Some(BooleanDefault::No);
        self
    }

    /// Make the question optional; its unattended answer is "yes".
    pub fn optional_default_yes(mut self) -> Self {
        self.default = Some(BooleanDefault::Yes);
        self
    }

    /// Make the question optional with an unattended answer derived from
    /// earlier answers, declaring those dependencies in the same operation so
    /// invalidation cannot drift from the closure.
    pub fn optional_default_from<F>(
        mut self,
        dependencies: impl IntoIterator<Item = StepId>,
        func: F,
    ) -> Self
    where
        F: Fn(&InstallContext) -> bool + 'static + Send + Sync,
    {
        self.add_dependencies(dependencies);
        self.default = Some(BooleanDefault::Derived(Box::new(func)));
        self
    }

    /// Make relevance depend on earlier answers and declare those dependencies
    /// in the same operation so invalidation cannot drift from the closure.
    pub fn relevant_when<F>(
        mut self,
        dependencies: impl IntoIterator<Item = StepId>,
        func: F,
    ) -> Self
    where
        F: Fn(&InstallContext) -> bool + 'static + Send + Sync,
    {
        self.add_dependencies(dependencies);
        self.should_ask_predicate = Some(Box::new(func));
        self
    }

    fn add_dependencies(&mut self, dependencies: impl IntoIterator<Item = StepId>) {
        for dependency in dependencies {
            if !self.dependencies.contains(&dependency) {
                self.dependencies.push(dependency);
            }
        }
    }
}

#[async_trait::async_trait]
impl WizardStep for BooleanQuestion {
    fn id(&self) -> StepId {
        self.id
    }

    fn description(&self) -> Option<&str> {
        self.description.as_deref().or(Some(&self.prompt))
    }

    fn should_ask(&self, context: &InstallContext) -> bool {
        if let Some(predicate) = &self.should_ask_predicate {
            predicate(context)
        } else {
            true
        }
    }

    fn depends_on(&self) -> &[StepId] {
        &self.dependencies
    }

    fn ask_policy(&self, context: &InstallContext) -> AskPolicy {
        match &self.default {
            // Confirm dialogs ignore preselection, so a required boolean has
            // no use for a default answer.
            None => AskPolicy::Required,
            Some(default) => AskPolicy::Optional {
                unattended_answer: Some(default.unattended_answer(context).to_string()),
            },
        }
    }

    /// Booleans have exactly two answers ("yes"/"no"). Validating here means
    /// `validate_imported_context` rejects hand-edited or stale configs whose
    /// booleans spell truth some other way ("true", "1", ...) instead of
    /// letting them silently read as false at execution time.
    fn validate(&self, _context: &InstallContext, answer: &str) -> Result<(), String> {
        match answer {
            "yes" | "no" => Ok(()),
            _ => Err(format!("expected \"yes\" or \"no\", got {answer:?}")),
        }
    }

    async fn run(&self, _context: &InstallContext) -> Result<StepOutcome> {
        let message = if let Some(desc) = &self.description {
            format!("{} {}\n\n{}", self.icon, self.prompt, desc)
        } else {
            format!("{} {}", self.icon, self.prompt)
        };

        // Use FzfWrapper's confirmation dialog for consistent yes/no prompts
        let result = FzfWrapper::builder().confirm(message).confirm_dialog()?;

        match result {
            ConfirmResult::Yes => Ok(StepOutcome::Answer("yes".to_string())),
            ConfirmResult::No => Ok(StepOutcome::Answer("no".to_string())),
            ConfirmResult::Cancelled => Ok(StepOutcome::Pause),
        }
    }
}
