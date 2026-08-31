use crate::arch::engine::{InstallContext, StepId, StepOutcome, WizardStep};
use crate::menu_utils::{ConfirmResult, FzfWrapper};
use crate::ui::nerd_font::NerdFont;
use anyhow::Result;

type ContextPredicate = dyn Fn(&InstallContext) -> bool + Send + Sync;

pub struct BooleanQuestion {
    id: StepId,
    prompt: String,
    icon: NerdFont,
    is_optional: bool,
    default_yes: bool,
    dynamic_default: Option<Box<ContextPredicate>>,
    should_ask_predicate: Option<Box<ContextPredicate>>,
    dependencies: Vec<StepId>,
}

impl BooleanQuestion {
    pub fn new(id: StepId, prompt: impl Into<String>, icon: NerdFont) -> Self {
        Self {
            id,
            prompt: prompt.into(),
            icon,
            is_optional: false,
            default_yes: false,
            dynamic_default: None,
            should_ask_predicate: None,
            dependencies: Vec::new(),
        }
    }

    pub fn optional(mut self) -> Self {
        self.is_optional = true;
        self
    }

    pub fn default_yes(mut self) -> Self {
        self.default_yes = true;
        self
    }

    /// Derive the default from earlier answers and declare those dependencies
    /// in the same operation so invalidation cannot drift from the closure.
    pub fn default_from<F>(
        mut self,
        dependencies: impl IntoIterator<Item = StepId>,
        func: F,
    ) -> Self
    where
        F: Fn(&InstallContext) -> bool + 'static + Send + Sync,
    {
        self.add_dependencies(dependencies);
        self.dynamic_default = Some(Box::new(func));
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
        Some(&self.prompt)
    }

    fn is_optional(&self) -> bool {
        self.is_optional
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

    fn get_default(&self, context: &InstallContext) -> Option<String> {
        let effective_default = if let Some(dynamic_func) = &self.dynamic_default {
            dynamic_func(context)
        } else {
            self.default_yes
        };
        Some(if effective_default {
            "yes".to_string()
        } else {
            "no".to_string()
        })
    }

    async fn run(&self, _context: &InstallContext) -> Result<StepOutcome> {
        // Use FzfWrapper's confirmation dialog for consistent yes/no prompts
        let result = FzfWrapper::builder()
            .confirm(format!("{} {}", self.icon, self.prompt))
            .confirm_dialog()?;

        match result {
            ConfirmResult::Yes => Ok(StepOutcome::Answer("yes".to_string())),
            ConfirmResult::No => Ok(StepOutcome::Answer("no".to_string())),
            ConfirmResult::Cancelled => Ok(StepOutcome::Pause),
        }
    }
}
