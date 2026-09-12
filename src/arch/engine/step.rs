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

/// How the wizard treats a step in the main flow.
///
/// Optionality is about flow placement, not relevance: optional steps are
/// configured through Advanced Options in the install flow and asked inline
/// in the setup flow. Whether a step applies to the current answers at all
/// is [`WizardStep::should_ask`]'s job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AskPolicy {
    /// Always ask; the wizard never answers this step on its own.
    Required,
    /// The step may run unattended. `unattended_answer` is recorded as its
    /// answer whenever no one is asked — applied automatically by the
    /// install flow, or offered as "Use Default" in the pause menu — and
    /// preselected when it is asked anyway; `None` leaves the step
    /// unanswered until the user visits it.
    Optional { unattended_answer: Option<String> },
}

impl AskPolicy {
    /// The answer recorded when this step runs unattended.
    pub fn unattended_answer(&self) -> Option<&str> {
        match self {
            Self::Required => None,
            Self::Optional { unattended_answer } => unattended_answer.as_deref(),
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

    /// Declares whether and how this step can run without user interaction.
    ///
    /// Optional steps are hidden from the install flow's main questions: the
    /// engine records their `unattended_answer` automatically when it has
    /// one, and otherwise only exposes them through the final review's
    /// Advanced Options menu and the pause menu. The setup flow asks
    /// optional steps inline, with "Use Default" in the pause menu when an
    /// unattended answer exists.
    ///
    /// Return [`AskPolicy::Required`] for steps with no sensible unattended
    /// answer. Declare every state this reads in
    /// [`WizardStep::depends_on`].
    fn ask_policy(&self, _context: &InstallContext) -> AskPolicy {
        AskPolicy::Required
    }

    /// Return a best-effort answer to preselect when this step is asked
    /// without a previous answer. Advisory only: only select dialogs honor
    /// it, it moves the initial cursor, and it is never recorded as the
    /// answer. Defaults to the policy's unattended answer, so an optional
    /// step that is asked despite its default starts with the cursor on
    /// it. Override to suggest a likely answer for required steps
    /// (detections, heuristics). Suggestions may use earlier answers and
    /// optional data populated by [`WizardStep::data_providers`], but must
    /// tolerate that data being absent and must declare those reads in
    /// [`WizardStep::depends_on`].
    fn preselect_answer(&self, context: &InstallContext) -> Option<String> {
        self.ask_policy(context)
            .unattended_answer()
            .map(str::to_string)
    }

    /// Returns the steps whose state this step is derived from.
    ///
    /// When any dependency changes, the engine removes this step's answer or
    /// completion marker transitively so it runs again. Declare every state
    /// that `run`, `should_ask`, `ask_policy`, `preselect_answer`, or
    /// `validate` reads for decision-making. Dependencies that are not part of the
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
