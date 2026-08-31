mod answer_graph;
mod presentation;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::io::Write;
use std::time::Duration;

use anyhow::{Result, bail};
use tokio::sync::mpsc;

use self::answer_graph::AnswerGraph;
use self::presentation::{
    AdvancedOption, FinalReviewAction, PauseMenuItem, ReviewItem, final_review_options,
};
use super::{InstallContext, Question, QuestionResult};
use crate::menu_utils::{ConfirmResult, FzfResult, FzfWrapper, Header, MenuPresentation};
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
pub enum EngineOutcome {
    /// The user accepted the final review.
    Completed(InstallContext),
    /// The user confirmed that the wizard should stop without applying changes.
    Aborted,
}

pub struct QuestionEngine {
    questions: Vec<Box<dyn Question>>,
    answer_graph: AnswerGraph,
    context: InstallContext,
    is_tty: bool,
    flow: FlowKind,
}

enum QuestionInteraction {
    Answered,
    Cancelled,
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

struct ProviderFailure {
    question_id: super::QuestionId,
    message: String,
}

struct ProviderRuntime {
    tasks: Vec<tokio::task::JoinHandle<()>>,
    failures: mpsc::UnboundedReceiver<ProviderFailure>,
    failures_by_question: HashMap<super::QuestionId, String>,
}

impl ProviderRuntime {
    fn failure_for(&mut self, question_id: super::QuestionId) -> Option<&str> {
        while let Ok(failure) = self.failures.try_recv() {
            self.failures_by_question
                .entry(failure.question_id)
                .or_insert(failure.message);
        }
        self.failures_by_question
            .get(&question_id)
            .map(String::as_str)
    }
}

impl Drop for ProviderRuntime {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

impl QuestionEngine {
    pub fn new(questions: Vec<Box<dyn Question>>) -> Result<Self> {
        Self::for_flow(FlowKind::Install, questions)
    }

    /// Create an engine for a specific wizard flow.
    pub fn for_flow(flow: FlowKind, questions: Vec<Box<dyn Question>>) -> Result<Self> {
        let answer_graph = AnswerGraph::new(&questions)?;
        Ok(Self {
            questions,
            answer_graph,
            context: InstallContext::new(),
            is_tty: is_tty_environment(),
            flow,
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
    pub async fn run(mut self) -> Result<EngineOutcome> {
        let mut providers = self.start_providers();

        loop {
            let Some(index) = self.find_next_question_index() else {
                match self.handle_final_review(&mut providers).await? {
                    FinalReviewResult::Continue => continue,
                    FinalReviewResult::Complete => {
                        return Ok(EngineOutcome::Completed(self.context));
                    }
                    FinalReviewResult::Abort => return Ok(EngineOutcome::Aborted),
                }
            };

            self.wait_until_ready(index, &mut providers).await?;
            loop {
                match self.ask_question(index).await? {
                    QuestionInteraction::Answered => break,
                    QuestionInteraction::Cancelled => {
                        match self.handle_navigation_menu(index).await? {
                            NavigationAction::Stay => continue,
                            NavigationAction::ContinueFlow => break,
                            NavigationAction::Abort => return Ok(EngineOutcome::Aborted),
                        }
                    }
                }
            }
        }
    }

    fn start_providers(&self) -> ProviderRuntime {
        let (failure_sender, failures) = mpsc::unbounded_channel();
        let mut tasks = Vec::new();
        for question in &self.questions {
            for provider in question.data_providers() {
                let context = self.context.clone();
                let question_id = question.id();
                let failure_sender = failure_sender.clone();
                tasks.push(tokio::spawn(async move {
                    if let Err(error) = provider.provide(&context).await {
                        let _ = failure_sender.send(ProviderFailure {
                            question_id,
                            message: error.to_string(),
                        });
                    }
                }));
            }
        }
        drop(failure_sender);
        ProviderRuntime {
            tasks,
            failures,
            failures_by_question: HashMap::new(),
        }
    }

    async fn wait_until_ready(&self, index: usize, providers: &mut ProviderRuntime) -> Result<()> {
        loop {
            let question_id = self.questions[index].id();
            if let Some(message) = providers.failure_for(question_id) {
                self.show_fatal_error(&format!(
                    "Data required for {question_id:?} could not be loaded: {message}"
                ))?;
            }
            if let Some(message) = self.questions[index].fatal_error_message(&self.context) {
                self.show_fatal_error(&message)?;
            }
            if self.questions[index].is_ready(&self.context) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    fn show_fatal_error(&self, message: &str) -> Result<()> {
        let full_message = format!(
            "{} Fatal Error\n\n{message}\n\nThe wizard cannot continue.",
            NerdFont::CrossCircle
        );
        if let Err(error) = FzfWrapper::message(&full_message) {
            eprintln!("Failed to display the fatal error dialog: {error}");
        }
        bail!("{message}")
    }

    async fn ask_question(&mut self, index: usize) -> Result<QuestionInteraction> {
        loop {
            self.clear_terminal()?;
            match self.questions[index].ask(&self.context).await? {
                QuestionResult::Answer(answer) => {
                    if let Err(message) = self.questions[index].validate(&self.context, &answer) {
                        FzfWrapper::message(&format!("{} {message}", NerdFont::Warning))?;
                        continue;
                    }

                    let id = self.questions[index].id();
                    self.answer_graph
                        .record_answer(&mut self.context, id, answer);
                    return Ok(QuestionInteraction::Answered);
                }
                QuestionResult::Cancelled => return Ok(QuestionInteraction::Cancelled),
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

    fn find_next_question_index(&mut self) -> Option<usize> {
        for index in 0..self.questions.len() {
            let question = &self.questions[index];
            if !question.should_ask(&self.context) {
                continue;
            }

            if question.is_optional() && self.flow == FlowKind::Install {
                if !self.context.is_answered(question.id())
                    && let Some(default) = question.get_default(&self.context)
                {
                    let id = question.id();
                    self.answer_graph
                        .record_answer(&mut self.context, id, default);
                }
                continue;
            }

            let id = question.id();
            if let Some(answer) = self.context.get_answer(&id) {
                if question.validate(&self.context, answer).is_err() {
                    self.answer_graph.drop_answer(&mut self.context, id);
                    return Some(index);
                }
            } else {
                return Some(index);
            }
        }
        None
    }

    async fn handle_navigation_menu(&mut self, current_index: usize) -> Result<NavigationAction> {
        let mut options = vec![
            PauseMenuItem::Resume,
            PauseMenuItem::ReviewAnswers,
            PauseMenuItem::GoBack,
        ];
        let current_question = &self.questions[current_index];
        if current_question.is_optional() && current_question.get_default(&self.context).is_some() {
            options.push(PauseMenuItem::UseDefault);
        }
        options.push(PauseMenuItem::Abort);

        let result = FzfWrapper::menu()
            .header(Header::fancy(self.flow.pause_menu_title()))
            .presentation(MenuPresentation::Padded)
            .select(options)?;

        match result {
            FzfResult::Selected(PauseMenuItem::Resume) => Ok(NavigationAction::Stay),
            FzfResult::Selected(PauseMenuItem::ReviewAnswers) => {
                self.review_answers(current_index).await?;
                Ok(NavigationAction::Stay)
            }
            FzfResult::Selected(PauseMenuItem::GoBack) => {
                let previous_index = self.previous_question_index(current_index);
                if previous_index == current_index {
                    return Ok(NavigationAction::Stay);
                }
                let id = self.questions[previous_index].id();
                self.answer_graph.drop_answer(&mut self.context, id);
                Ok(NavigationAction::ContinueFlow)
            }
            FzfResult::Selected(PauseMenuItem::UseDefault) => {
                let question = &self.questions[current_index];
                let Some(default) = question.get_default(&self.context) else {
                    return Ok(NavigationAction::Stay);
                };
                self.answer_graph
                    .record_answer(&mut self.context, question.id(), default);
                Ok(NavigationAction::ContinueFlow)
            }
            FzfResult::Selected(PauseMenuItem::Abort) => {
                if self.confirm_abort()? {
                    Ok(NavigationAction::Abort)
                } else {
                    Ok(NavigationAction::Stay)
                }
            }
            _ => Ok(NavigationAction::Stay),
        }
    }

    async fn handle_final_review(
        &mut self,
        providers: &mut ProviderRuntime,
    ) -> Result<FinalReviewResult> {
        let result = FzfWrapper::builder()
            .header(Header::fancy(self.flow.final_review_title()))
            .prompt("Select")
            .responsive_layout()
            .select(final_review_options(self.flow, &self.context))?;

        let FzfResult::Selected(option) = result else {
            return Ok(FinalReviewResult::Continue);
        };
        match option.action {
            FinalReviewAction::Complete => Ok(FinalReviewResult::Complete),
            FinalReviewAction::ReviewAnswers => {
                self.review_answers(self.questions.len()).await?;
                Ok(FinalReviewResult::Continue)
            }
            FinalReviewAction::AdvancedOptions => {
                if let Some(index) = self.select_advanced_option()? {
                    self.wait_until_ready(index, providers).await?;
                    let _ = self.ask_question(index).await?;
                }
                Ok(FinalReviewResult::Continue)
            }
            FinalReviewAction::Abort => {
                if self.confirm_abort()? {
                    Ok(FinalReviewResult::Abort)
                } else {
                    Ok(FinalReviewResult::Continue)
                }
            }
        }
    }

    async fn review_answers(&mut self, before_index: usize) -> Result<()> {
        while let Some(index) = self.select_answer_to_review(before_index)? {
            let _ = self.ask_question(index).await?;
        }
        Ok(())
    }

    fn select_answer_to_review(&self, before_index: usize) -> Result<Option<usize>> {
        let mut items = vec![ReviewItem::Continue];
        for (index, question) in self.questions.iter().enumerate().take(before_index) {
            if let Some(answer) = self.context.get_answer(&question.id()) {
                items.push(ReviewItem::Question {
                    index,
                    id: question.id(),
                    description: question.description().unwrap_or_default().to_string(),
                    answer: answer.clone(),
                    is_sensitive: question.is_sensitive(),
                });
            }
        }

        if items.len() == 1 {
            FzfWrapper::message(&format!("{} No answers to review yet.", NerdFont::Info))?;
            return Ok(None);
        }

        let result = FzfWrapper::builder()
            .header(Header::fancy("Select a question to modify"))
            .prompt("Search")
            .responsive_layout()
            .select(items)?;
        match result {
            FzfResult::Selected(ReviewItem::Question { index, .. }) => Ok(Some(index)),
            _ => Ok(None),
        }
    }

    fn select_advanced_option(&self) -> Result<Option<usize>> {
        let result = FzfWrapper::builder().header("Advanced Options").select(
            AdvancedOption::from_questions(&self.questions, &self.context),
        )?;
        match result {
            FzfResult::Selected(AdvancedOption::Question { index, .. }) => Ok(Some(index)),
            _ => Ok(None),
        }
    }

    fn previous_question_index(&self, current_index: usize) -> usize {
        if current_index == 0 {
            return 0;
        }

        let mut index = current_index - 1;
        while index > 0
            && (!self.questions[index].should_ask(&self.context)
                || self.questions[index].is_info_only())
        {
            index -= 1;
        }
        index
    }

    fn confirm_abort(&self) -> Result<bool> {
        Ok(FzfWrapper::confirm(self.flow.abort_confirmation())? == ConfirmResult::Yes)
    }
}

fn is_tty_environment() -> bool {
    std::env::var("TERM")
        .map(|term| term == "linux")
        .unwrap_or(false)
        || (std::env::var("DISPLAY").is_err() && std::env::var("WAYLAND_DISPLAY").is_err())
}
