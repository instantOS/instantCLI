mod presentation;
mod step_graph;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::io::Write;

use anyhow::{Result, bail};

use self::presentation::{
    AdvancedOption, FinalReviewAction, FinalReviewOption, PauseMenuItem, ReviewItem,
    final_review_options,
};
use self::step_graph::StepGraph;
use super::read_audit;
use super::step::AskPolicy;
use super::{InstallContext, StepOutcome, WizardStep};
use crate::menu_utils::{ConfirmResult, FzfWrapper, Header, MenuCursor};
use crate::ui::nerd_font::NerdFont;

/// Which wizard is driving the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowKind {
    /// Full installation wizard. Optional questions are configured through
    /// Advanced Options and otherwise receive their defaults.
    Install,
    /// Focused setup wizard. Optional questions are asked in the main flow.
    Setup,
}

impl FlowKind {
    fn final_review_title(self) -> &'static str {
        match self {
            Self::Install => "Installation Configuration Complete",
            Self::Setup => "Setup Configuration Complete",
        }
    }

    fn pause_menu_title(self) -> &'static str {
        match self {
            Self::Install => "Installation Paused",
            Self::Setup => "Setup Paused",
        }
    }

    fn abort_confirmation(self) -> &'static str {
        match self {
            Self::Install => "Are you sure you want to abort the installation?",
            Self::Setup => "Are you sure you want to abort setup?",
        }
    }
}

/// Result of running an interactive question flow.
pub enum WizardOutcome {
    /// The user accepted the final review.
    Completed(Box<InstallContext>),
    /// The user confirmed that the wizard should stop without applying changes.
    Aborted,
}

pub struct WizardEngine {
    steps: Vec<Box<dyn WizardStep>>,
    step_graph: StepGraph,
    context: InstallContext,
    is_tty: bool,
    flow: FlowKind,
    /// Cursor memory for the final review and advanced options menus.
    ///
    /// These are fields, not locals, because `handle_final_review` hands
    /// control back to the main run loop whenever the flow must settle
    /// again (or the user dismisses the review with Esc); the next visit
    /// must restore the row the user came from.
    review_cursor: MenuCursor,
    advanced_cursor: MenuCursor,
}

enum StepInteraction {
    Completed,
    Paused,
    Back {
        message: Option<String>,
    },
    Revisit {
        step: super::StepId,
        message: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StepReadiness {
    Ready,
    Irrelevant,
}

enum NavigationAction {
    Stay,
    ContinueFlow,
    Abort,
}

enum FinalReviewResult {
    Continue,
    Complete,
    Abort,
}

struct ProviderRuntime {
    tasks: Vec<ProviderTask>,
    failures_by_question: HashMap<super::StepId, String>,
}

struct ProviderTask {
    question_id: super::StepId,
    handle: tokio::task::JoinHandle<std::result::Result<(), String>>,
}

impl ProviderRuntime {
    fn has_tasks_for(&self, question_id: super::StepId) -> bool {
        self.tasks
            .iter()
            .any(|task| task.question_id == question_id)
    }

    async fn finish_tasks_for(&mut self, question_id: super::StepId) -> Result<()> {
        while let Some(index) = self
            .tasks
            .iter()
            .position(|task| task.question_id == question_id)
        {
            self.finish_task_at(index).await;
        }
        if let Some(message) = self.failures_by_question.get(&question_id) {
            bail!("{message}");
        }
        Ok(())
    }

    async fn finish_all(&mut self) {
        while !self.tasks.is_empty() {
            self.finish_task_at(0).await;
        }
    }

    async fn finish_task_at(&mut self, index: usize) {
        let task = self.tasks.swap_remove(index);
        let failure = match task.handle.await {
            Ok(Ok(())) => return,
            Ok(Err(message)) => message,
            Err(error) => format!("data provider task failed: {error}"),
        };
        self.failures_by_question
            .entry(task.question_id)
            .or_insert(failure);
    }
}

impl Drop for ProviderRuntime {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.handle.abort();
        }
    }
}

impl WizardEngine {
    pub fn new(steps: Vec<Box<dyn WizardStep>>) -> Result<Self> {
        Self::for_flow(FlowKind::Install, steps)
    }

    /// Create an engine for a specific wizard flow.
    pub fn for_flow(flow: FlowKind, steps: Vec<Box<dyn WizardStep>>) -> Result<Self> {
        let step_graph = StepGraph::new(&steps)?;
        Ok(Self {
            steps,
            step_graph,
            context: InstallContext::new(),
            is_tty: is_tty_environment(),
            flow,
            review_cursor: MenuCursor::new(),
            advanced_cursor: MenuCursor::new(),
        })
    }

    pub fn with_context(mut self, context: InstallContext) -> Self {
        self.context = context;
        self
    }

    /// Run the wizard until the user completes or explicitly aborts it.
    ///
    /// Data providers are started here so every entry point gets identical
    /// initialization and callers cannot accidentally omit it.
    pub async fn run(mut self) -> Result<WizardOutcome> {
        self.normalize_context();
        let mut providers = self.start_providers();

        loop {
            let Some(index) = self.find_next_step_index() else {
                // Resolve every provider before the final review so provider-
                // driven relevance (notably mirror fallback) is reflected in
                // the context that will be saved. Failures for already valid
                // answers remain dormant unless that question must be asked.
                providers.finish_all().await;
                self.normalize_context();
                if self.find_next_step_index().is_some() {
                    continue;
                }
                match self.handle_final_review(&mut providers).await? {
                    FinalReviewResult::Continue => continue,
                    FinalReviewResult::Complete => {
                        return Ok(WizardOutcome::Completed(Box::new(self.context)));
                    }
                    FinalReviewResult::Abort => return Ok(WizardOutcome::Aborted),
                }
            };

            if matches!(
                self.wait_until_ready(index, &mut providers).await?,
                StepReadiness::Irrelevant
            ) {
                continue;
            }
            loop {
                match self.run_step(index).await? {
                    StepInteraction::Completed => break,
                    StepInteraction::Back { message } => {
                        self.show_navigation_message(message)?;
                        self.go_back_from(index);
                        break;
                    }
                    StepInteraction::Revisit { step, message } => {
                        self.show_navigation_message(message)?;
                        self.revisit_from(index, step)?;
                        break;
                    }
                    StepInteraction::Paused => match self.handle_navigation_menu(index).await? {
                        NavigationAction::Stay => continue,
                        NavigationAction::ContinueFlow => break,
                        NavigationAction::Abort => return Ok(WizardOutcome::Aborted),
                    },
                }
            }
        }
    }

    fn start_providers(&self) -> ProviderRuntime {
        let mut tasks = Vec::new();
        for step in &self.steps {
            for provider in step.data_providers() {
                let context = self.context.clone();
                let question_id = step.id();
                tasks.push(ProviderTask {
                    question_id,
                    handle: tokio::spawn(async move {
                        provider
                            .provide(&context)
                            .await
                            .map_err(|error| format!("{error:#}"))
                    }),
                });
            }
        }
        ProviderRuntime {
            tasks,
            failures_by_question: HashMap::new(),
        }
    }

    async fn wait_until_ready(
        &mut self,
        index: usize,
        providers: &mut ProviderRuntime,
    ) -> Result<StepReadiness> {
        let question_id = self.steps[index].id();
        let had_providers = providers.has_tasks_for(question_id);
        if let Err(error) = providers.finish_tasks_for(question_id).await {
            self.show_fatal_error(&format!(
                "Data required for {question_id:?} could not be loaded: {error}"
            ))?;
        }

        // A provider may make a question irrelevant while populating its data
        // (the mirror provider uses this for its fallback path).
        if !self.steps[index].should_ask(&self.context) {
            self.step_graph
                .drop_step_state(&mut self.context, question_id);
            return Ok(StepReadiness::Irrelevant);
        }
        if let Some(message) = self.steps[index].fatal_error_message(&self.context) {
            self.show_fatal_error(&message)?;
        }
        if self.steps[index].is_ready(&self.context) {
            return Ok(StepReadiness::Ready);
        }
        let source = if had_providers {
            "provider completed without supplying"
        } else {
            "question has no provider for"
        };
        self.show_fatal_error(&format!(
            "Data unavailable for {question_id:?}: {source} the required data"
        ))
    }

    fn show_fatal_error<T>(&self, message: &str) -> Result<T> {
        let full_message = format!(
            "{} Fatal Error\n\n{message}\n\nThe wizard cannot continue.",
            NerdFont::CrossCircle
        );
        if let Err(error) = FzfWrapper::message(&full_message) {
            eprintln!("Failed to display the fatal error dialog: {error}");
        }
        bail!("{message}")
    }

    async fn run_step(&mut self, index: usize) -> Result<StepInteraction> {
        loop {
            self.clear_terminal()?;
            match self.steps[index].run(&self.context).await? {
                StepOutcome::Answer(answer) => {
                    if let Err(message) = read_audit::hook(&*self.steps[index], "validate", || {
                        self.steps[index].validate(&self.context, &answer)
                    }) {
                        FzfWrapper::message(&format!("{} {message}", NerdFont::Warning))?;
                        continue;
                    }

                    let id = self.steps[index].id();
                    self.step_graph.record_answer(&mut self.context, id, answer);
                    return Ok(StepInteraction::Completed);
                }
                StepOutcome::Completed => {
                    let id = self.steps[index].id();
                    self.step_graph.record_completion(&mut self.context, id);
                    return Ok(StepInteraction::Completed);
                }
                StepOutcome::Retry(message) => {
                    FzfWrapper::message(&format!("{} {message}", NerdFont::Warning))?;
                }
                StepOutcome::Back { message } => return Ok(StepInteraction::Back { message }),
                StepOutcome::Revisit { step, message } => {
                    return Ok(StepInteraction::Revisit { step, message });
                }
                StepOutcome::Pause => return Ok(StepInteraction::Paused),
            }
        }
    }

    fn clear_terminal(&self) -> std::io::Result<()> {
        if self.is_tty {
            print!("\x1B[2J\x1B[1;1H");
            std::io::stdout().flush()?;
        }
        Ok(())
    }

    fn find_next_step_index(&mut self) -> Option<usize> {
        for index in 0..self.steps.len() {
            let step = &self.steps[index];
            if !read_audit::hook(&**step, "should_ask", || step.should_ask(&self.context)) {
                let id = step.id();
                self.step_graph.drop_step_state(&mut self.context, id);
                continue;
            }

            // Optional steps never run in the install flow's main questions.
            // Their unattended answers are applied here so the context
            // settles the same way no matter which entry point resumes the
            // flow.
            if self.flow == FlowKind::Install
                && let AskPolicy::Optional { unattended_answer } =
                    read_audit::hook(&**step, "ask_policy", || step.ask_policy(&self.context))
            {
                let id = step.id();
                if !self.context.is_step_completed(id)
                    && let Some(answer) = unattended_answer
                {
                    self.step_graph.record_answer(&mut self.context, id, answer);
                }
                continue;
            }

            let id = step.id();
            if let Some(answer) = self.context.get_answer(&id) {
                if read_audit::hook(&**step, "validate", || step.validate(&self.context, answer))
                    .is_err()
                {
                    self.step_graph.drop_step_state(&mut self.context, id);
                    return Some(index);
                }
            } else if self.context.is_step_completed(id) {
                if step.completion_is_current(&self.context) {
                    continue;
                }
                self.step_graph.drop_step_state(&mut self.context, id);
                return Some(index);
            } else {
                return Some(index);
            }
        }
        None
    }

    /// Reconcile imported answers with the current question graph.
    ///
    /// Root answers can be validated directly. Dependent step state must carry
    /// provenance proving that it was recorded against the current upstream
    /// values.
    fn normalize_context(&mut self) {
        for index in 0..self.steps.len() {
            let step = &self.steps[index];
            let id = step.id();

            if !read_audit::hook(&**step, "should_ask", || step.should_ask(&self.context)) {
                self.step_graph.drop_step_state(&mut self.context, id);
                continue;
            }

            if let Some(answer) = self.context.get_answer(&id).cloned() {
                if read_audit::hook(&**step, "validate", || {
                    step.validate(&self.context, &answer)
                })
                .is_err()
                    || !self.step_graph.step_state_is_current(&self.context, id)
                {
                    self.step_graph.drop_step_state(&mut self.context, id);
                } else {
                    self.step_graph.record_answer(&mut self.context, id, answer);
                }
            } else if self.context.completed_steps.contains(&id) {
                if step.completion_is_current(&self.context)
                    && self.step_graph.step_state_is_current(&self.context, id)
                {
                    self.step_graph.record_completion(&mut self.context, id);
                } else {
                    self.step_graph.drop_step_state(&mut self.context, id);
                }
            }
        }
    }

    async fn handle_navigation_menu(&mut self, current_index: usize) -> Result<NavigationAction> {
        let mut options = vec![
            PauseMenuItem::Resume,
            PauseMenuItem::ReviewAnswers,
            PauseMenuItem::GoBack,
        ];
        let current_step = &self.steps[current_index];
        let policy = read_audit::hook(&**current_step, "ask_policy", || {
            current_step.ask_policy(&self.context)
        });
        if matches!(
            policy,
            AskPolicy::Optional {
                unattended_answer: Some(_)
            }
        ) {
            options.push(PauseMenuItem::UseDefault);
        }
        options.push(PauseMenuItem::Abort);

        let result = FzfWrapper::menu()
            .header(Header::fancy(self.flow.pause_menu_title()))
            .items(options)
            .padded()
            .select_one()?;

        match result {
            crate::menu_utils::DialogOutcome::Submitted(PauseMenuItem::Resume) => {
                Ok(NavigationAction::Stay)
            }
            crate::menu_utils::DialogOutcome::Submitted(PauseMenuItem::ReviewAnswers) => {
                self.review_answers(current_index).await?;
                Ok(NavigationAction::Stay)
            }
            crate::menu_utils::DialogOutcome::Submitted(PauseMenuItem::GoBack) => {
                self.go_back_from(current_index);
                Ok(NavigationAction::ContinueFlow)
            }
            crate::menu_utils::DialogOutcome::Submitted(PauseMenuItem::UseDefault) => {
                let AskPolicy::Optional {
                    unattended_answer: Some(unattended_answer),
                } = policy
                else {
                    // Unreachable while the menu only offers Use Default for
                    // optional steps with an unattended answer; stay rather
                    // than panic if that invariant ever changes.
                    return Ok(NavigationAction::Stay);
                };
                let id = current_step.id();
                self.step_graph
                    .record_answer(&mut self.context, id, unattended_answer);
                Ok(NavigationAction::ContinueFlow)
            }
            crate::menu_utils::DialogOutcome::Submitted(PauseMenuItem::Abort) => {
                if self.confirm_abort()? {
                    Ok(NavigationAction::Abort)
                } else {
                    Ok(NavigationAction::Stay)
                }
            }
            crate::menu_utils::DialogOutcome::Cancelled => Ok(NavigationAction::Stay),
        }
    }

    async fn handle_final_review(
        &mut self,
        providers: &mut ProviderRuntime,
    ) -> Result<FinalReviewResult> {
        let mut in_advanced_options = false;

        loop {
            // Re-answer edits and advanced-option answers can invalidate
            // downstream answers (including required ones) or leave optional
            // defaults unapplied. Settle the flow again before showing a menu
            // so the user can only complete from a fully answered context.
            if self.find_next_step_index().is_some() {
                return Ok(FinalReviewResult::Continue);
            }
            if in_advanced_options {
                let Some(index) = self.select_advanced_option()? else {
                    in_advanced_options = false;
                    continue;
                };
                self.run_advanced_option(index, providers).await?;
            } else {
                let Some(option) = self.select_final_review()? else {
                    return Ok(FinalReviewResult::Continue);
                };
                match option.action {
                    FinalReviewAction::Complete => return Ok(FinalReviewResult::Complete),
                    FinalReviewAction::ReviewAnswers => {
                        self.review_answers(self.steps.len()).await?;
                    }
                    FinalReviewAction::AdvancedOptions => in_advanced_options = true,
                    FinalReviewAction::Abort => {
                        if self.confirm_abort()? {
                            return Ok(FinalReviewResult::Abort);
                        }
                    }
                }
            }
        }
    }

    fn select_final_review(&mut self) -> Result<Option<FinalReviewOption>> {
        let options = final_review_options(self.flow, &self.context);
        let mut selection = FzfWrapper::builder()
            .header(Header::fancy(self.flow.final_review_title()))
            .prompt("Select")
            .responsive_layout()
            .items(options.clone());
        if let Some(index) = self.review_cursor.initial_index(&options) {
            selection = selection.initial_index(index);
        }

        match selection.padded().select_one()? {
            crate::menu_utils::DialogOutcome::Submitted(option) => {
                self.review_cursor.update(&option, &options);
                Ok(Some(option))
            }
            crate::menu_utils::DialogOutcome::Cancelled => Ok(None),
        }
    }

    /// Answer one advanced option, then return to the advanced menu.
    async fn run_advanced_option(
        &mut self,
        index: usize,
        providers: &mut ProviderRuntime,
    ) -> Result<()> {
        if !matches!(
            self.wait_until_ready(index, providers).await?,
            StepReadiness::Ready
        ) {
            return Ok(());
        }
        match self.run_step(index).await? {
            StepInteraction::Completed | StepInteraction::Paused => {}
            StepInteraction::Back { message } => {
                self.show_navigation_message(message)?;
                self.go_back_from(index);
            }
            StepInteraction::Revisit { step, message } => {
                self.show_navigation_message(message)?;
                self.revisit_from(index, step)?;
            }
        }
        Ok(())
    }

    async fn review_answers(&mut self, before_index: usize) -> Result<()> {
        let mut cursor = MenuCursor::new();
        while let Some(index) = self.select_answer_to_review(before_index, &mut cursor)? {
            match self.run_step(index).await? {
                StepInteraction::Completed => {}
                StepInteraction::Paused => return Ok(()),
                StepInteraction::Back { message } => {
                    self.show_navigation_message(message)?;
                    self.go_back_from(index);
                    return Ok(());
                }
                StepInteraction::Revisit { step, message } => {
                    self.show_navigation_message(message)?;
                    self.revisit_from(index, step)?;
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    fn select_answer_to_review(
        &self,
        before_index: usize,
        cursor: &mut MenuCursor,
    ) -> Result<Option<usize>> {
        let mut items = vec![ReviewItem::Continue];
        for (index, step) in self.steps.iter().enumerate().take(before_index) {
            if let Some(answer) = self.context.get_answer(&step.id()) {
                items.push(ReviewItem::Answer {
                    index,
                    id: step.id(),
                    description: step.description().unwrap_or_default().to_string(),
                    answer: answer.clone(),
                    is_sensitive: step.is_sensitive(),
                });
            }
        }

        if items.len() == 1 {
            FzfWrapper::message(&format!("{} No answers to review yet.", NerdFont::Info))?;
            return Ok(None);
        }

        let mut selection = FzfWrapper::builder()
            .header(Header::fancy("Select a question to modify"))
            .prompt("Search")
            .responsive_layout()
            .items(items.clone());
        if let Some(index) = cursor.initial_index(&items) {
            selection = selection.initial_index(index);
        }

        match selection.padded().select_one()? {
            crate::menu_utils::DialogOutcome::Submitted(item) => {
                cursor.update(&item, &items);
                match item {
                    ReviewItem::Answer { index, .. } => Ok(Some(index)),
                    ReviewItem::Continue => Ok(None),
                }
            }
            crate::menu_utils::DialogOutcome::Cancelled => Ok(None),
        }
    }

    fn select_advanced_option(&mut self) -> Result<Option<usize>> {
        let options = AdvancedOption::from_steps(&self.steps, &self.context);
        let mut selection = FzfWrapper::builder()
            .header(Header::fancy("Advanced Options"))
            .items(options.clone());
        if let Some(index) = self.advanced_cursor.initial_index(&options) {
            selection = selection.initial_index(index);
        }

        match selection.padded().select_one()? {
            crate::menu_utils::DialogOutcome::Submitted(option) => {
                self.advanced_cursor.update(&option, &options);
                match option {
                    AdvancedOption::Answer { index, .. } => Ok(Some(index)),
                    AdvancedOption::Back => Ok(None),
                }
            }
            crate::menu_utils::DialogOutcome::Cancelled => Ok(None),
        }
    }

    fn previous_step_index(&self, current_index: usize) -> usize {
        if current_index == 0 {
            return 0;
        }

        let mut index = current_index - 1;
        while index > 0
            && (!self.steps[index].should_ask(&self.context) || self.steps[index].is_info_only())
        {
            index -= 1;
        }
        index
    }

    fn go_back_from(&mut self, current_index: usize) {
        let previous_index = self.previous_step_index(current_index);
        if previous_index != current_index {
            let id = self.steps[previous_index].id();
            self.step_graph.drop_step_state(&mut self.context, id);
        }
    }

    fn revisit_from(&mut self, current_index: usize, target: super::StepId) -> Result<()> {
        let Some(target_index) = self.steps.iter().position(|step| step.id() == target) else {
            bail!("step {target:?} requested navigation to a step outside this wizard");
        };
        if target_index >= current_index {
            bail!("step {target:?} must precede the step requesting navigation");
        }
        self.step_graph.drop_step_state(&mut self.context, target);
        Ok(())
    }

    fn show_navigation_message(&self, message: Option<String>) -> Result<()> {
        if let Some(message) = message {
            FzfWrapper::message(&format!("{} {message}", NerdFont::Warning))?;
        }
        Ok(())
    }

    fn confirm_abort(&self) -> Result<bool> {
        Ok(FzfWrapper::confirm(self.flow.abort_confirmation())? == ConfirmResult::Yes)
    }
}

/// Validate an imported context against the question graph for flows that
/// execute recorded answers without re-asking them (`ins arch exec`).
///
/// This is the exec-path counterpart of [`WizardEngine::normalize_context`]:
/// instead of dropping unusable state and asking again, the first problem
/// becomes a fatal error so an unattended run can never act on it.
///
/// Every answer present in the context must belong to a relevant step, pass
/// that step's `validate`, and have been recorded against the dependency
/// values currently in the context. Missing answers are deliberately not an
/// error here: a step's relevance can depend on provider data that only
/// exists inside a wizard run (for example the mirror-region fetch-failure
/// flag), so demanding answers would reject contexts the wizard itself
/// produced. The execution code fails loudly at its use sites on the
/// answers it truly requires.
pub fn validate_imported_context(
    steps: &[Box<dyn WizardStep>],
    context: &InstallContext,
) -> Result<()> {
    let graph = StepGraph::new(steps)?;
    for step in steps {
        let id = step.id();
        if !read_audit::hook(&**step, "should_ask", || step.should_ask(context)) {
            if context.get_answer(&id).is_some() || context.is_step_completed(id) {
                bail!("the stored state for {id:?} is irrelevant to the selected configuration")
            }
            continue;
        }

        let Some(answer) = context.get_answer(&id) else {
            continue;
        };

        read_audit::hook(&**step, "validate", || step.validate(context, answer)).map_err(
            |message| anyhow::anyhow!("the stored answer for {id:?} is invalid: {message}"),
        )?;

        if !graph.step_state_is_current(context, id) {
            bail!(
                "the stored answer for {id:?} is stale: it was recorded against different dependency answers"
            );
        }
    }
    Ok(())
}

fn is_tty_environment() -> bool {
    std::env::var("TERM")
        .map(|term| term == "linux")
        .unwrap_or(false)
        || (std::env::var("DISPLAY").is_err() && std::env::var("WAYLAND_DISPLAY").is_err())
}
