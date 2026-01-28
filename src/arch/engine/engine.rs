use anyhow::Result;

use crate::menu_utils::{ConfirmResult, FzfPreview, FzfResult, FzfSelectable, FzfWrapper, Header};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::context::InstallContext;
use super::question::{Question, QuestionResult};

pub struct QuestionEngine {
    questions: Vec<Box<dyn Question>>,
    pub context: InstallContext,
    is_tty: bool,
}

#[derive(Clone)]
enum PauseMenuItem {
    Resume,
    ReviewAnswers,
    GoBack,
    AbortInstallation,
}

impl PauseMenuItem {
    fn preview(&self) -> FzfPreview {
        match self {
            PauseMenuItem::Resume => PreviewBuilder::new()
                .header(NerdFont::Play, "Resume Installation")
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
            PauseMenuItem::AbortInstallation => format!(
                "{} Abort Installation",
                format_icon_colored(NerdFont::CrossCircle, colors::RED)
            ),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview()
    }

    fn fzf_key(&self) -> String {
        match self {
            PauseMenuItem::Resume => "resume".to_string(),
            PauseMenuItem::ReviewAnswers => "review_answers".to_string(),
            PauseMenuItem::GoBack => "go_back".to_string(),
            PauseMenuItem::AbortInstallation => "abort_installation".to_string(),
        }
    }
}

#[derive(Clone)]
enum FinalReviewItem {
    Install,
    ReviewAnswers,
    AdvancedOptions,
    AbortInstallation,
}

impl FinalReviewItem {
    fn preview(&self) -> FzfPreview {
        match self {
            FinalReviewItem::Install => PreviewBuilder::new()
                .header(NerdFont::Download, "Start Installation")
                .text("Apply the selected configuration.")
                .blank()
                .line(
                    colors::GREEN,
                    Some(NerdFont::Check),
                    "Begins the install process.",
                )
                .build(),
            FinalReviewItem::ReviewAnswers => review_answers_preview(),
            FinalReviewItem::AdvancedOptions => PreviewBuilder::new()
                .header(NerdFont::Sliders, "Advanced Options")
                .text("Configure optional steps before installing.")
                .blank()
                .line(colors::TEAL, None, "Optional questions and tweaks.")
                .build(),
            FinalReviewItem::AbortInstallation => abort_installation_preview(),
        }
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
        self.preview()
    }

    fn fzf_key(&self) -> String {
        match self {
            FinalReviewItem::Install => "install".to_string(),
            FinalReviewItem::ReviewAnswers => "review_answers".to_string(),
            FinalReviewItem::AdvancedOptions => "advanced_options".to_string(),
            FinalReviewItem::AbortInstallation => "abort_installation".to_string(),
        }
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
        .header(NerdFont::CrossCircle, "Abort Installation")
        .text("Stop the installer and return to the shell.")
        .blank()
        .line(
            colors::RED,
            Some(NerdFont::Warning),
            "Exits before installation starts.",
        )
        .build()
}

impl QuestionEngine {
    pub fn new(questions: Vec<Box<dyn Question>>) -> Self {
        Self {
            questions,
            context: InstallContext::new(),
            is_tty: is_tty_environment(),
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

        let continue_opt = format!("{} Continue with installation", NerdFont::ArrowRight);
        review_items.push(continue_opt.clone());

        for q in self.questions.iter().take(current_index) {
            if let Some(ans) = self.context.get_answer(&q.id()) {
                let display_ans = if q.is_sensitive() {
                    "******"
                } else {
                    ans.as_str()
                };
                review_items.push(format!("{} {:?}: {}", NerdFont::Check, q.id(), display_ans));
            }
        }

        if review_items.len() == 1 {
            FzfWrapper::message(&format!("{} No answers to review yet.", NerdFont::Info))?;
            return Ok(None);
        }

        let review = FzfWrapper::builder()
            .header("Select a question to modify")
            .select(review_items)?;

        if let FzfResult::Selected(selection) = review {
            if selection == continue_opt {
                return Ok(None);
            }

            // Format: "ICON QuestionId: Answer"
            let parts: Vec<&str> = selection.splitn(3, ' ').collect();
            if parts.len() >= 2 {
                let id_str = parts[1].trim_end_matches(':');
                if let Some(new_index) = self
                    .questions
                    .iter()
                    .position(|q| format!("{:?}", q.id()) == id_str)
                {
                    return Ok(Some(new_index));
                }
            }
        }
        Ok(None)
    }

    fn handle_go_back(&self, mut index: usize) -> usize {
        if index > 0 {
            index -= 1;
            while index > 0 && !self.questions[index].should_ask(&self.context) {
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
                        self.show_fatal_error_and_exit(&error_msg)?;
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
                                        self.context.answers.insert(id, answer);
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
    fn show_fatal_error_and_exit(&self, message: &str) -> Result<()> {
        let full_message = format!(
            "{} Fatal Error\n\n{}\n\nThe installation cannot continue.",
            NerdFont::CrossCircle,
            message
        );
        // Show fatal error dialog
        let _ = FzfWrapper::message(&full_message);
        std::process::exit(1);
    }

    fn find_next_question_index(&mut self) -> Option<usize> {
        for (i, q) in self.questions.iter().enumerate() {
            if !q.should_ask(&self.context) {
                continue;
            }

            // Skip optional questions in the main flow
            if q.is_optional() {
                // If not answered, try to set default
                if !self.context.is_answered(q.id())
                    && let Some(default) = q.get_default(&self.context)
                {
                    self.context.answers.insert(q.id(), default);
                }
                continue;
            }

            if let Some(ans) = self.context.get_answer(&q.id()) {
                if q.validate(&self.context, ans).is_err() {
                    self.context.answers.remove(&q.id());
                    return Some(i);
                }
            } else {
                return Some(i);
            }
        }
        None
    }

    async fn handle_navigation_menu(&mut self, current_idx: usize) -> Result<bool> {
        let options = vec![
            PauseMenuItem::Resume,
            PauseMenuItem::ReviewAnswers,
            PauseMenuItem::GoBack,
            PauseMenuItem::AbortInstallation,
        ];
        let nav = FzfWrapper::builder()
            .header(Header::fancy("Installation Paused"))
            .prompt("Select")
            .args(fzf_mocha_args())
            .responsive_layout()
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
                    self.context.answers.remove(&q_id);
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
        let options = vec![
            FinalReviewItem::Install,
            FinalReviewItem::ReviewAnswers,
            FinalReviewItem::AdvancedOptions,
            FinalReviewItem::AbortInstallation,
        ];
        let nav = FzfWrapper::builder()
            .header(Header::fancy("Installation Configuration Complete"))
            .prompt("Select")
            .args(fzf_mocha_args())
            .responsive_layout()
            .select(options)?;

        match nav {
            FzfResult::Selected(FinalReviewItem::Install) => Ok(true),
            FzfResult::Selected(FinalReviewItem::ReviewAnswers) => {
                while let Some(review_idx) = self.handle_review(self.questions.len())? {
                    self.force_ask_question(review_idx).await?;
                }
                Ok(false)
            }
            FzfResult::Selected(FinalReviewItem::AdvancedOptions) => {
                if let Some(adv_idx) = self.handle_advanced_options()? {
                    self.force_ask_question(adv_idx).await?;
                }
                Ok(false)
            }
            FzfResult::Selected(FinalReviewItem::AbortInstallation) => {
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
                            self.context.answers.insert(id, answer);
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
    use crate::arch::engine::DataKey;

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
