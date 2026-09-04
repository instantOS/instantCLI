use anyhow::Result;

use crate::menu_utils::DialogOutcome;

use super::context::{DataKey, InstallContext};
use super::types::StepId;

/// Result of running one interactive wizard step.
pub enum StepOutcome {
    /// Store configuration data and complete the step.
    Answer(String),
    /// Complete a side-effect or informational step without inventing an answer.
    Completed,
    /// Show the message and run the current step again.
    Retry(String),
    /// Return directly to the previous relevant step, optionally explaining why.
    Back { message: Option<String> },
    /// Invalidate and revisit a specific earlier step.
    Revisit {
        step: StepId,
        message: Option<String>,
    },
    /// Open the wizard's pause menu.
    Pause,
}

impl StepOutcome {
    /// Map a submitted dialog value into a step answer. Cancelling the
    /// dialog opens the wizard's pause menu.
    pub fn from_dialog<T>(result: DialogOutcome<T>, extract: impl FnOnce(T) -> String) -> Self {
        match result {
            DialogOutcome::Submitted(value) => StepOutcome::Answer(extract(value)),
            DialogOutcome::Cancelled => StepOutcome::Pause,
        }
    }

    pub fn back() -> Self {
        Self::Back { message: None }
    }

    pub fn revisit(step: StepId, message: impl Into<String>) -> Self {
        Self::Revisit {
            step,
            message: Some(message.into()),
        }
    }
}

/// Trait for providing async data to the install context
#[async_trait::async_trait]
pub trait AsyncDataProvider: Send + Sync {
    /// Fetches data and updates the context
    async fn provide(&self, context: &InstallContext) -> Result<()>;

    /// Returns an optional annotation provider for this data provider
    fn annotation_provider(&self) -> Option<Box<dyn crate::arch::annotations::AnnotationProvider>> {
        None
    }

    /// Helper to annotate and save a list of items to the context
    fn save_list<K, T>(&self, context: &InstallContext, items: Vec<T>)
    where
        T: crate::menu_utils::FzfSelectable + Clone + Send + Sync + Ord + 'static,
        K: DataKey<Value = Vec<crate::arch::annotations::AnnotatedValue<T>>>,
        Self: Sized,
    {
        let provider = self.annotation_provider();
        let annotated = crate::arch::annotations::annotate_list(provider.as_deref(), items);
        context.set::<K>(annotated);
    }
}

/// A navigable unit in the interactive configuration wizard.
#[async_trait::async_trait]
pub trait WizardStep: Send + Sync {
    fn id(&self) -> StepId;

    /// Returns data keys that must exist before this step can run.
    fn required_data_keys(&self) -> Vec<String> {
        vec![]
    }

    /// Returns true if the step is ready to run.
    fn is_ready(&self, context: &InstallContext) -> bool {
        let keys = self.required_data_keys();
        if keys.is_empty() {
            return true;
        }
        let data = context.data.lock().unwrap();
        keys.iter().all(|k| data.contains_key(k))
    }

    /// Run the step and report an explicit navigation or completion outcome.
    async fn run(&self, context: &InstallContext) -> Result<StepOutcome>;

    /// Returns true if the step is relevant/active given the current context.
    ///
    /// Ordering contract: predicates may only read answers of questions that
    /// appear *earlier* in the wizard's step list, and must tolerate their
    /// absence (falling back to a sensible default). The engine does not
    /// enforce reads at runtime. The step graph validates every declared
    /// dependency and its ordering, so implementations must keep
    /// [`WizardStep::depends_on`] in sync with their predicates and validators.
    fn should_ask(&self, _context: &InstallContext) -> bool {
        true
    }

    /// Returns true if the answer should be masked in the review UI
    fn is_sensitive(&self) -> bool {
        false
    }

    /// Returns true if the question is optional and should be skipped in the main flow
    fn is_optional(&self) -> bool {
        false
    }

    /// Returns true if this step is an informational message or warning
    /// and should be skipped when navigating backwards
    fn is_info_only(&self) -> bool {
        false
    }

    /// A short human-readable description of what this step is for.
    /// Shown in the review menu preview when browsing answers.
    fn description(&self) -> Option<&str> {
        None
    }

    /// Validate the answer. Returns Ok(()) if valid, or Err(message) if invalid.
    fn validate(&self, _context: &InstallContext, _answer: &str) -> Result<(), String> {
        Ok(())
    }

    /// Recheck whether an answerless completion marker still reflects reality.
    /// Side-effect and check steps backed by mutable external state should
    /// override this; informational steps can keep the default.
    fn completion_is_current(&self, _context: &InstallContext) -> bool {
        true
    }

    /// Returns a list of data providers required by this step.
    fn data_providers(&self) -> Vec<Box<dyn AsyncDataProvider>> {
        vec![]
    }

    /// Returns the default value for this step if one exists.
    fn get_default(&self, _context: &InstallContext) -> Option<String> {
        None
    }

    /// Returns the steps whose state this step is derived from.
    ///
    /// When any dependency changes, the engine removes this step's answer or
    /// completion marker transitively so it runs again. Declare every state
    /// that `run`, `should_ask`, `get_default`, or `validate`
    /// reads for decision-making. Dependencies that are not part of the
    /// current wizard's step list are permitted (e.g. pre-seeded contexts)
    /// but must still appear earlier in the list when they are present.
    fn depends_on(&self) -> &[StepId] {
        &[]
    }

    /// Returns a fatal error message if this step cannot proceed due to a required
    /// data provider failure. Override this for steps where provider failure is fatal
    /// (e.g., disk selection). Return None for questions that handle failures gracefully
    /// (e.g., mirror regions with fallback).
    fn fatal_error_message(&self, _context: &InstallContext) -> Option<String> {
        None
    }
}
