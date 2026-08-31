use anyhow::Result;
use std::collections::HashMap;

use crate::menu_utils::{
    ConfirmResult, FzfPreview, FzfResult, FzfSelectable, FzfWrapper, Header, MenuPresentation,
};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::context::InstallContext;
use super::question::{Question, QuestionResult};
use super::summary::{
    InstallSummary, PartitioningKind, build_install_summary, build_setup_summary,
};
use super::types::QuestionId;

/// Which wizard is driving the engine. Controls whether optional questions
/// are asked in the main flow and how the final review screen is presented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowKind {
    /// Full installation wizard. Optional questions are only reachable via
    /// Advanced Options; the final review talks about installing.
    Install,
    /// Focused setup wizard (e.g. `ins arch setup`). Every provided question
    /// is asked in the main flow; the final review talks about applying setup.
    Setup,
}

pub struct QuestionEngine {
    questions: Vec<Box<dyn Question>>,
    pub context: InstallContext,
    is_tty: bool,
    flow: FlowKind,
}

#[derive(Clone)]
enum PauseMenuItem {
    Resume,
    ReviewAnswers,
    GoBack,
    UseDefault,
    AbortInstallation,
}

impl std::fmt::Display for PauseMenuItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PauseMenuItem::Resume => write!(f, "resume"),
            PauseMenuItem::ReviewAnswers => write!(f, "review_answers"),
            PauseMenuItem::GoBack => write!(f, "go_back"),
            PauseMenuItem::UseDefault => write!(f, "use_default"),
            PauseMenuItem::AbortInstallation => write!(f, "abort_installation"),
        }
    }
}

impl PauseMenuItem {
    fn preview(&self) -> FzfPreview {
        match self {
            PauseMenuItem::Resume => PreviewBuilder::new()
                .header(NerdFont::Play, "Resume")
                .text("Continue the current question flow.")
                .blank()
                .line(
                    colors::GREEN,
                    Some(NerdFont::Check),
                    "Keeps all current answers.",
                )
                .build(),
            PauseMenuItem::ReviewAnswers => review_answers_preview(),
            PauseMenuItem::GoBack => PreviewBuilder::new()
                .header(NerdFont::ArrowLeft, "Go Back")
                .text("Return to the previous question.")
                .blank()
                .line(
                    colors::PEACH,
                    Some(NerdFont::ArrowLeft),
                    "Re-answer the previous step.",
                )
                .build(),
            PauseMenuItem::UseDefault => PreviewBuilder::new()
                .header(NerdFont::Check, "Use Default")
                .text("Continue without answering; the default value will be applied.")
                .blank()
                .line(
                    colors::GREEN,
                    Some(NerdFont::Check),
                    "The wizard continues with the default answer.",
                )
                .build(),
            PauseMenuItem::AbortInstallation => abort_installation_preview(),
        }
    }
}

impl FzfSelectable for PauseMenuItem {
    fn fzf_display_text(&self) -> String {
        match self {
            PauseMenuItem::Resume => {
                format!(
                    "{} Resume",
                    format_icon_colored(NerdFont::Play, colors::GREEN)
                )
            }
            PauseMenuItem::ReviewAnswers => format!(
                "{} Review Answers",
                format_icon_colored(NerdFont::List, colors::BLUE)
            ),
            PauseMenuItem::GoBack => format!("{} Go Back", format_back_icon()),
            PauseMenuItem::UseDefault => format!(
                "{} Use Default",
                format_icon_colored(NerdFont::Check, colors::TEAL)
            ),
            PauseMenuItem::AbortInstallation => format!(
                "{} Abort",
                format_icon_colored(NerdFont::CrossCircle, colors::RED)
            ),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview()
    }
}

#[derive(Clone)]
enum FinalReviewItem {
    Install,
    ReviewAnswers,
    AdvancedOptions,
    AbortInstallation,
}

impl std::fmt::Display for FinalReviewItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FinalReviewItem::Install => write!(f, "install"),
            FinalReviewItem::ReviewAnswers => write!(f, "review_answers"),
            FinalReviewItem::AdvancedOptions => write!(f, "advanced_options"),
            FinalReviewItem::AbortInstallation => write!(f, "abort_installation"),
        }
    }
}

#[derive(Clone)]
struct FinalReviewOption {
    kind: FinalReviewItem,
    label: String,
    preview: FzfPreview,
}

impl FinalReviewOption {
    fn new(kind: FinalReviewItem, label: impl Into<String>, preview: FzfPreview) -> Self {
        Self {
            kind,
            label: label.into(),
            preview,
        }
    }
}

impl FzfSelectable for FinalReviewOption {
    fn fzf_display_text(&self) -> String {
        self.label.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview.clone()
    }

    fn fzf_key(&self) -> String {
        self.kind.fzf_key()
    }
}

impl FzfSelectable for FinalReviewItem {
    fn fzf_display_text(&self) -> String {
        match self {
            FinalReviewItem::Install => format!(
                "{} Install",
                format_icon_colored(NerdFont::Download, colors::GREEN)
            ),
            FinalReviewItem::ReviewAnswers => format!(
                "{} Review Answers",
                format_icon_colored(NerdFont::List, colors::BLUE)
            ),
            FinalReviewItem::AdvancedOptions => format!(
                "{} Advanced Options",
                format_icon_colored(NerdFont::Sliders, colors::PEACH)
            ),
            FinalReviewItem::AbortInstallation => format!(
                "{} Abort Installation",
                format_icon_colored(NerdFont::CrossCircle, colors::RED)
            ),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        FzfPreview::None
    }
}

fn review_answers_preview() -> FzfPreview {
    PreviewBuilder::new()
        .header(NerdFont::List, "Review Answers")
        .text("Browse and edit your previous responses.")
        .blank()
        .line(colors::TEAL, None, "Pick a question to revisit.")
        .build()
}

fn abort_installation_preview() -> FzfPreview {
    PreviewBuilder::new()
        .header(NerdFont::CrossCircle, "Abort")
        .text("Stop the wizard and return to the shell.")
        .blank()
        .line(
            colors::RED,
            Some(NerdFont::Warning),
            "Exits before any changes are made.",
        )
        .build()
}

#[derive(Clone)]
enum ReviewItem {
    Continue,
    Question {
        index: usize,
        id: String,
        description: String,
        answer: String,
        is_sensitive: bool,
    },
}

impl FzfSelectable for ReviewItem {
    fn fzf_display_text(&self) -> String {
        match self {
            ReviewItem::Continue => format!(
                "{} Continue",
                format_icon_colored(NerdFont::ArrowRight, colors::GREEN)
            ),
            ReviewItem::Question {
                id,
                answer,
                is_sensitive,
                ..
            } => {
                let display_ans = if *is_sensitive {
                    "******".to_string()
                } else {
                    answer.clone()
                };
                let truncated = if display_ans.len() > 50 {
                    format!("{}…", &display_ans[..47])
                } else {
                    display_ans
                };
                format!(
                    "{} {}: {}",
                    format_icon_colored(NerdFont::Check, colors::TEAL),
                    id,
                    truncated,
                )
            }
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            ReviewItem::Continue => PreviewBuilder::new()
                .header(NerdFont::ArrowRight, "Continue")
                .text("Resume the wizard.")
                .blank()
                .line(
                    colors::GREEN,
                    Some(NerdFont::Check),
                    "All reviewed answers will be kept.",
                )
                .build(),
            ReviewItem::Question {
                id,
                description,
                answer,
                is_sensitive,
                ..
            } => {
                let display_ans = if *is_sensitive {
                    "******".to_string()
                } else {
                    answer.clone()
                };
                let mut builder = PreviewBuilder::new().header(NerdFont::Question, id);
                if !description.is_empty() {
                    builder = builder.subtext(description);
                }
                builder
                    .blank()
                    .field("Current Answer", &display_ans)
                    .blank()
                    .line(colors::TEAL, None, "Select to re-answer this question.")
                    .build()
            }
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            ReviewItem::Continue => "continue".to_string(),
            ReviewItem::Question { index, .. } => format!("q_{}", index),
        }
    }
}

fn build_final_review_preview(item: &FinalReviewItem, summary: &InstallSummary) -> FzfPreview {
    match item {
        FinalReviewItem::Install => {
            let mut builder = PreviewBuilder::new()
                .header(NerdFont::Download, "Start Installation")
                .text("Apply the selected configuration.")
                .blank();

            if summary.partitioning_kind == PartitioningKind::Automatic {
                builder = builder
                    .line(
                        colors::YELLOW,
                        Some(NerdFont::Warning),
                        "Selected disk will be erased.",
                    )
                    .blank();
            }

            builder.raw(&summary.text).build()
        }
        FinalReviewItem::ReviewAnswers => PreviewBuilder::new()
            .header(NerdFont::List, "Review Answers")
            .text("Browse and edit your previous responses.")
            .blank()
            .raw(&summary.text)
            .build(),
        FinalReviewItem::AdvancedOptions => PreviewBuilder::new()
            .header(NerdFont::Sliders, "Advanced Options")
            .text("Configure optional steps before installing.")
            .blank()
            .raw(&summary.text)
            .build(),
        FinalReviewItem::AbortInstallation => abort_installation_preview(),
    }
}

impl QuestionEngine {
    /// Record an answer and invalidate any stored answers derived from it.
    ///
    /// All answer mutations must go through here (or [`Self::drop_answer`])
    /// so that `depends_on` invalidation stays consistent. Setting the same
    /// value again is a no-op and keeps dependent answers intact.
    ///
    /// Takes the question list and answer map separately so callers iterating
    /// over `self.questions` can split-borrow the context.
    fn record_answer(
        questions: &[Box<dyn Question>],
        answers: &mut HashMap<QuestionId, String>,
        id: QuestionId,
        answer: String,
    ) {
        if answers.get(&id) == Some(&answer) {
            return;
        }
        answers.insert(id.clone(), answer);
        Self::invalidate_dependents(questions, answers, &id);
    }

    /// Remove an answer and invalidate any stored answers derived from it.
    fn drop_answer(
        questions: &[Box<dyn Question>],
        answers: &mut HashMap<QuestionId, String>,
        id: &QuestionId,
    ) {
        if answers.remove(id).is_none() {
            return;
        }
        Self::invalidate_dependents(questions, answers, id);
    }

    /// Transitively remove answers of questions that declared a dependency on
    /// `changed` (directly or through another invalidated answer).
    fn invalidate_dependents(
        questions: &[Box<dyn Question>],
        answers: &mut HashMap<QuestionId, String>,
        changed: &QuestionId,
    ) {
        let mut queue = vec![changed.clone()];
        while let Some(current) = queue.pop() {
            for question in questions {
                let dependent_id = question.id();
                if dependent_id == current || !answers.contains_key(&dependent_id) {
                    continue;
                }
                if question.depends_on().contains(&current)
                    && answers.remove(&dependent_id).is_some()
                {
                    queue.push(dependent_id);
                }
            }
        }
    }

    pub fn new(questions: Vec<Box<dyn Question>>) -> Self {
        Self::for_flow(FlowKind::Install, questions)
    }

    /// Create an engine for a specific wizard flow.
    ///
    /// The flow decides whether optional questions are asked in the main flow
    /// and how the final review screen is worded. See [`FlowKind`].
    pub fn for_flow(flow: FlowKind, questions: Vec<Box<dyn Question>>) -> Self {
        Self {
            questions,
            context: InstallContext::new(),
            is_tty: is_tty_environment(),
            flow,
        }
    }

    pub fn initialize_providers(&self) {
        for question in &self.questions {
            for provider in question.data_providers() {
                let context = self.context.clone();
                tokio::spawn(async move {
                    if let Err(e) = provider.provide(&context).await {
                        eprintln!("Data provider failed: {}", e);
                    }
                });
            }
        }
    }

    fn handle_review(&self, current_index: usize) -> Result<Option<usize>> {
        let mut review_items = Vec::new();

        review_items.push(ReviewItem::Continue);

        for (i, q) in self.questions.iter().enumerate().take(current_index) {
            if let Some(ans) = self.context.get_answer(&q.id()) {
                review_items.push(ReviewItem::Question {
                    index: i,
                    id: format!("{:?}", q.id()),
                    description: q.description().unwrap_or("").to_string(),
                    answer: ans.clone(),
                    is_sensitive: q.is_sensitive(),
                });
            }
        }

        if review_items.len() == 1 {
            FzfWrapper::message(&format!("{} No answers to review yet.", NerdFont::Info))?;
            return Ok(None);
        }

        let review = FzfWrapper::builder()
            .header(Header::fancy("Select a question to modify"))
            .prompt("Search")
            .responsive_layout()
            .select(review_items)?;

        match review {
            FzfResult::Selected(ReviewItem::Continue) => Ok(None),
            FzfResult::Selected(ReviewItem::Question { index, .. }) => Ok(Some(index)),
            _ => Ok(None),
        }
    }

    fn handle_go_back(&self, mut index: usize) -> usize {
        if index > 0 {
            index -= 1;
            while index > 0
                && (!self.questions[index].should_ask(&self.context)
                    || self.questions[index].is_info_only())
            {
                index -= 1;
            }
        }
        index
    }

    pub async fn run(mut self) -> Result<InstallContext> {
        loop {
            match self.find_next_question_index() {
                Some(idx) => {
                    while !self.questions[idx].is_ready(&self.context) {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }

                    // Check for fatal provider errors before asking
                    if let Some(error_msg) = self.questions[idx].fatal_error_message(&self.context)
                    {
                        self.show_fatal_error_and_exit(&error_msg);
                    }

                    loop {
                        // Clear screen if running in TTY to avoid artifacts
                        if self.is_tty {
                            print!("\x1B[2J\x1B[1;1H");
                            use std::io::Write;
                            let _ = std::io::stdout().flush();
                        }

                        let result = self.questions[idx].ask(&self.context).await?;
                        match result {
                            QuestionResult::Answer(answer) => {
                                match self.questions[idx].validate(&self.context, &answer) {
                                    Ok(()) => {
                                        let id = self.questions[idx].id();
                                        QuestionEngine::record_answer(
                                            &self.questions,
                                            &mut self.context.answers,
                                            id,
                                            answer,
                                        );
                                        break;
                                    }
                                    Err(msg) => {
                                        FzfWrapper::message(&format!(
                                            "{} {}",
                                            NerdFont::Warning,
                                            msg
                                        ))?;
                                    }
                                }
                            }

                            QuestionResult::Cancelled => {
                                if self.handle_navigation_menu(idx).await? {
                                    break;
                                }
                            }
                        }
                    }
                }
                None => {
                    if self.handle_final_review().await? {
                        break;
                    }
                }
            }
        }

        Ok(self.context.clone())
    }

    /// Show a fatal error message and exit the installer
    fn show_fatal_error_and_exit(&self, message: &str) -> ! {
        let full_message = format!(
            "{} Fatal Error\n\n{}\n\nThe installation cannot continue.",
            NerdFont::CrossCircle,
            message
        );
        let _ = FzfWrapper::message(&full_message);
        std::process::exit(1);
    }

    fn find_next_question_index(&mut self) -> Option<usize> {
        for (i, q) in self.questions.iter().enumerate() {
            if !q.should_ask(&self.context) {
                continue;
            }

            // In the install flow, optional questions are skipped in the main
            // flow (their default is applied) and only reachable via Advanced
            // Options. Other flows ask them like required questions.
            if q.is_optional() && self.flow == FlowKind::Install {
                // If not answered, try to set default
                if !self.context.is_answered(q.id())
                    && let Some(default) = q.get_default(&self.context)
                {
                    let id = q.id();
                    QuestionEngine::record_answer(
                        &self.questions,
                        &mut self.context.answers,
                        id,
                        default,
                    );
                }
                continue;
            }

            if let Some(ans) = self.context.get_answer(&q.id()) {
                if q.validate(&self.context, ans).is_err() {
                    let id = q.id();
                    QuestionEngine::drop_answer(&self.questions, &mut self.context.answers, &id);
                    return Some(i);
                }
            } else {
                return Some(i);
            }
        }
        None
    }

    async fn handle_navigation_menu(&mut self, current_idx: usize) -> Result<bool> {
        let mut options = vec![
            PauseMenuItem::Resume,
            PauseMenuItem::ReviewAnswers,
            PauseMenuItem::GoBack,
        ];

        // Skipping only makes sense for optional questions with a default to
        // fall back on. In the install flow those are never asked inline, so
        // this entry effectively only shows up in other flows.
        let current_question = &self.questions[current_idx];
        if current_question.is_optional() && current_question.get_default(&self.context).is_some() {
            options.push(PauseMenuItem::UseDefault);
        }
        options.push(PauseMenuItem::AbortInstallation);

        let nav = FzfWrapper::menu()
            .header(Header::fancy(self.pause_menu_title()))
            .presentation(MenuPresentation::Padded)
            .select(options)?;

        match nav {
            FzfResult::Selected(PauseMenuItem::Resume) => Ok(false),
            FzfResult::Selected(PauseMenuItem::ReviewAnswers) => {
                while let Some(review_idx) = self.handle_review(current_idx)? {
                    self.force_ask_question(review_idx).await?;
                }
                Ok(false)
            }
            FzfResult::Selected(PauseMenuItem::GoBack) => {
                let prev_idx = self.handle_go_back(current_idx);
                if prev_idx != current_idx {
                    let q_id = self.questions[prev_idx].id();
                    QuestionEngine::drop_answer(&self.questions, &mut self.context.answers, &q_id);
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            FzfResult::Selected(PauseMenuItem::UseDefault) => {
                if let Some(default) = self.questions[current_idx].get_default(&self.context) {
                    let q_id = self.questions[current_idx].id();
                    QuestionEngine::record_answer(
                        &self.questions,
                        &mut self.context.answers,
                        q_id,
                        default,
                    );
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            FzfResult::Selected(PauseMenuItem::AbortInstallation) => {
                if let Ok(ConfirmResult::Yes) =
                    FzfWrapper::confirm("Are you sure you want to abort?")
                {
                    std::process::exit(0);
                }
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    async fn handle_final_review(&mut self) -> Result<bool> {
        let options = self.final_review_options();
        let nav = FzfWrapper::builder()
            .header(Header::fancy(self.final_review_title()))
            .prompt("Select")
            .responsive_layout()
            .select(options)?;

        match nav {
            FzfResult::Selected(option) => match option.kind {
                FinalReviewItem::Install => Ok(true),
                FinalReviewItem::ReviewAnswers => {
                    while let Some(review_idx) = self.handle_review(self.questions.len())? {
                        self.force_ask_question(review_idx).await?;
                    }
                    Ok(false)
                }
                FinalReviewItem::AdvancedOptions => {
                    if let Some(adv_idx) = self.handle_advanced_options()? {
                        self.force_ask_question(adv_idx).await?;
                    }
                    Ok(false)
                }
                FinalReviewItem::AbortInstallation => {
                    if let Ok(ConfirmResult::Yes) =
                        FzfWrapper::confirm("Are you sure you want to abort?")
                    {
                        std::process::exit(0);
                    }
                    Ok(false)
                }
            },
            _ => Ok(false),
        }
    }

    fn final_review_title(&self) -> &'static str {
        match self.flow {
            FlowKind::Install => "Installation Configuration Complete",
            FlowKind::Setup => "Setup Configuration Complete",
        }
    }

    fn pause_menu_title(&self) -> &'static str {
        match self.flow {
            FlowKind::Install => "Installation Paused",
            FlowKind::Setup => "Setup Paused",
        }
    }

    fn final_review_options(&self) -> Vec<FinalReviewOption> {
        match self.flow {
            FlowKind::Install => {
                let summary = build_install_summary(&self.context);
                vec![
                    FinalReviewOption::new(
                        FinalReviewItem::Install,
                        "Install",
                        build_final_review_preview(&FinalReviewItem::Install, &summary),
                    ),
                    FinalReviewOption::new(
                        FinalReviewItem::ReviewAnswers,
                        "Review Answers",
                        build_final_review_preview(&FinalReviewItem::ReviewAnswers, &summary),
                    ),
                    FinalReviewOption::new(
                        FinalReviewItem::AdvancedOptions,
                        "Advanced Options",
                        build_final_review_preview(&FinalReviewItem::AdvancedOptions, &summary),
                    ),
                    FinalReviewOption::new(
                        FinalReviewItem::AbortInstallation,
                        "Abort",
                        build_final_review_preview(&FinalReviewItem::AbortInstallation, &summary),
                    ),
                ]
            }
            FlowKind::Setup => {
                let summary_text = build_setup_summary(&self.context);
                vec![
                    FinalReviewOption::new(
                        FinalReviewItem::Install,
                        "Apply Setup",
                        PreviewBuilder::new()
                            .header(NerdFont::Download, "Apply Setup")
                            .text("Configure this system with instantOS.")
                            .blank()
                            .raw(&summary_text)
                            .build(),
                    ),
                    FinalReviewOption::new(
                        FinalReviewItem::ReviewAnswers,
                        "Review Answers",
                        PreviewBuilder::new()
                            .header(NerdFont::List, "Review Answers")
                            .text("Browse and edit your previous responses.")
                            .blank()
                            .raw(&summary_text)
                            .build(),
                    ),
                    FinalReviewOption::new(
                        FinalReviewItem::AbortInstallation,
                        "Abort",
                        abort_installation_preview(),
                    ),
                ]
            }
        }
    }

    fn handle_advanced_options(&self) -> Result<Option<usize>> {
        let mut options = Vec::new();
        let back_opt = format!("{} Back", NerdFont::ArrowLeft);
        options.push(back_opt.clone());

        for q in self.questions.iter() {
            if q.is_optional() && q.should_ask(&self.context) {
                let status = if self.context.is_answered(q.id()) {
                    let ans = self.context.get_answer(&q.id()).unwrap();
                    format!("{:?} (Current: {})", q.id(), ans)
                } else {
                    format!("{:?}", q.id())
                };
                options.push(format!("{} {}", NerdFont::Gear, status));
            }
        }

        let result = FzfWrapper::builder()
            .header("Advanced Options")
            .select(options)?;

        if let FzfResult::Selected(selection) = result {
            if selection == back_opt {
                return Ok(None);
            }

            // Parse selection to find question index
            // Format: "ICON QuestionId (Current: ...)" or "ICON QuestionId"
            // We can iterate and check which question ID matches the string
            for (i, q) in self.questions.iter().enumerate() {
                if q.is_optional() {
                    let id_str = format!("{:?}", q.id());
                    if selection.contains(&id_str) {
                        return Ok(Some(i));
                    }
                }
            }
        }

        Ok(None)
    }

    async fn force_ask_question(&mut self, idx: usize) -> Result<()> {
        loop {
            let result = self.questions[idx].ask(&self.context).await?;
            match result {
                QuestionResult::Answer(answer) => {
                    match self.questions[idx].validate(&self.context, &answer) {
                        Ok(()) => {
                            let id = self.questions[idx].id();
                            QuestionEngine::record_answer(
                                &self.questions,
                                &mut self.context.answers,
                                id,
                                answer,
                            );
                            break;
                        }
                        Err(msg) => {
                            FzfWrapper::message(&format!("{} {}", NerdFont::Warning, msg))?;
                        }
                    }
                }
                QuestionResult::Cancelled => break,
            }
        }
        Ok(())
    }
}

fn is_tty_environment() -> bool {
    std::env::var("TERM").map(|t| t == "linux").unwrap_or(false)
        || (std::env::var("DISPLAY").is_err() && std::env::var("WAYLAND_DISPLAY").is_err())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arch::engine::{DataKey, QuestionId};
    use anyhow::Result;

    struct TestKey;
    impl DataKey for TestKey {
        type Value = String;
        const KEY: &'static str = "test_key";
    }

    struct IntKey;
    impl DataKey for IntKey {
        type Value = i32;
        const KEY: &'static str = "int_key";
    }

    /// Minimal optional question with a default, for flow behavior tests.
    struct StubOptionalQuestion {
        id: QuestionId,
        default: Option<String>,
    }

    #[async_trait::async_trait]
    impl Question for StubOptionalQuestion {
        fn id(&self) -> QuestionId {
            self.id.clone()
        }

        async fn ask(&self, _context: &InstallContext) -> Result<QuestionResult> {
            Ok(QuestionResult::Answer("answered".to_string()))
        }

        fn is_optional(&self) -> bool {
            true
        }

        fn get_default(&self, _context: &InstallContext) -> Option<String> {
            self.default.clone()
        }
    }

    #[test]
    fn install_flow_applies_optional_defaults_without_asking() {
        let question = StubOptionalQuestion {
            id: QuestionId::Autologin,
            default: Some("no".to_string()),
        };
        let mut engine = QuestionEngine::for_flow(FlowKind::Install, vec![Box::new(question)]);

        assert_eq!(engine.find_next_question_index(), None);
        assert_eq!(
            engine
                .context
                .get_answer(&QuestionId::Autologin)
                .map(String::as_str),
            Some("no")
        );
    }

    #[test]
    fn setup_flow_asks_optional_questions_in_main_flow() {
        let question = StubOptionalQuestion {
            id: QuestionId::Autologin,
            default: Some("no".to_string()),
        };
        let mut engine = QuestionEngine::for_flow(FlowKind::Setup, vec![Box::new(question)]);

        assert_eq!(engine.find_next_question_index(), Some(0));
        assert!(engine.context.get_answer(&QuestionId::Autologin).is_none());
    }

    /// Minimal question whose answer is derived from declared dependencies,
    /// for invalidation-cascade tests.
    struct StubDependentQuestion {
        id: QuestionId,
        deps: Vec<QuestionId>,
    }

    #[async_trait::async_trait]
    impl Question for StubDependentQuestion {
        fn id(&self) -> QuestionId {
            self.id.clone()
        }

        async fn ask(&self, _context: &InstallContext) -> Result<QuestionResult> {
            Ok(QuestionResult::Answer("answered".to_string()))
        }

        fn depends_on(&self) -> Vec<QuestionId> {
            self.deps.clone()
        }
    }

    #[test]
    fn changing_an_answer_invalidates_dependents_transitively() {
        let disk = StubDependentQuestion {
            id: QuestionId::Disk,
            deps: vec![],
        };
        let partition = StubDependentQuestion {
            id: QuestionId::DualBootPartition,
            deps: vec![QuestionId::Disk],
        };
        let size = StubDependentQuestion {
            id: QuestionId::DualBootSize,
            deps: vec![QuestionId::DualBootPartition],
        };

        let mut engine = QuestionEngine::for_flow(
            FlowKind::Setup,
            vec![Box::new(disk), Box::new(partition), Box::new(size)],
        );
        let answers = &mut engine.context.answers;

        QuestionEngine::record_answer(
            &engine.questions,
            answers,
            QuestionId::Disk,
            "/dev/nvme0n1".to_string(),
        );
        QuestionEngine::record_answer(
            &engine.questions,
            answers,
            QuestionId::DualBootPartition,
            "/dev/nvme0n1p3".to_string(),
        );
        QuestionEngine::record_answer(
            &engine.questions,
            answers,
            QuestionId::DualBootSize,
            "800".to_string(),
        );

        // User re-answers Disk in review: the partition answer derived from
        // the old disk and the size derived from that partition must go.
        QuestionEngine::record_answer(
            &engine.questions,
            answers,
            QuestionId::Disk,
            "/dev/sda".to_string(),
        );

        assert!(answers.get(&QuestionId::DualBootPartition).is_none());
        assert!(answers.get(&QuestionId::DualBootSize).is_none());
        assert_eq!(
            answers.get(&QuestionId::Disk).map(String::as_str),
            Some("/dev/sda")
        );
    }

    #[test]
    fn re_answering_with_the_same_value_keeps_dependents() {
        let disk = StubDependentQuestion {
            id: QuestionId::Disk,
            deps: vec![],
        };
        let partition = StubDependentQuestion {
            id: QuestionId::DualBootPartition,
            deps: vec![QuestionId::Disk],
        };

        let mut engine =
            QuestionEngine::for_flow(FlowKind::Setup, vec![Box::new(disk), Box::new(partition)]);
        let answers = &mut engine.context.answers;

        QuestionEngine::record_answer(
            &engine.questions,
            answers,
            QuestionId::Disk,
            "/dev/sda".to_string(),
        );
        QuestionEngine::record_answer(
            &engine.questions,
            answers,
            QuestionId::DualBootPartition,
            "/dev/sda2".to_string(),
        );
        QuestionEngine::record_answer(
            &engine.questions,
            answers,
            QuestionId::Disk,
            "/dev/sda".to_string(),
        );

        assert_eq!(
            answers
                .get(&QuestionId::DualBootPartition)
                .map(String::as_str),
            Some("/dev/sda2")
        );
    }

    #[test]
    fn removing_an_answer_invalidates_dependents() {
        let disk = StubDependentQuestion {
            id: QuestionId::Disk,
            deps: vec![],
        };
        let partition = StubDependentQuestion {
            id: QuestionId::DualBootPartition,
            deps: vec![QuestionId::Disk],
        };

        let mut engine =
            QuestionEngine::for_flow(FlowKind::Setup, vec![Box::new(disk), Box::new(partition)]);
        let answers = &mut engine.context.answers;

        QuestionEngine::record_answer(
            &engine.questions,
            answers,
            QuestionId::Disk,
            "/dev/sda".to_string(),
        );
        QuestionEngine::record_answer(
            &engine.questions,
            answers,
            QuestionId::DualBootPartition,
            "/dev/sda2".to_string(),
        );
        QuestionEngine::drop_answer(&engine.questions, answers, &QuestionId::Disk);

        assert!(answers.get(&QuestionId::DualBootPartition).is_none());
    }

    #[test]
    fn test_install_context_typemap() {
        let context = InstallContext::new();

        context.set::<TestKey>("hello".to_string());
        context.set::<IntKey>(42);

        assert_eq!(context.get::<TestKey>(), Some("hello".to_string()));
        assert_eq!(context.get::<IntKey>(), Some(42));

        // Test missing key
        struct MissingKey;
        impl DataKey for MissingKey {
            type Value = bool;
            const KEY: &'static str = "missing";
        }
        assert_eq!(context.get::<MissingKey>(), None);
    }
}
