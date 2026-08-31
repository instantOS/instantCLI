use std::fmt;

use super::super::{InstallContext, Question, QuestionId};
use super::FlowKind;
use crate::arch::engine::summary::{
    InstallSummary, PartitioningKind, build_install_summary, build_setup_summary,
};
use crate::menu_utils::{FzfPreview, FzfSelectable};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

#[derive(Clone)]
pub(super) enum PauseMenuItem {
    Resume,
    ReviewAnswers,
    GoBack,
    UseDefault,
    Abort,
}

impl fmt::Display for PauseMenuItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let key = match self {
            Self::Resume => "resume",
            Self::ReviewAnswers => "review_answers",
            Self::GoBack => "go_back",
            Self::UseDefault => "use_default",
            Self::Abort => "abort",
        };
        f.write_str(key)
    }
}

impl FzfSelectable for PauseMenuItem {
    fn fzf_display_text(&self) -> String {
        match self {
            Self::Resume => format!(
                "{} Resume",
                format_icon_colored(NerdFont::Play, colors::GREEN)
            ),
            Self::ReviewAnswers => format!(
                "{} Review Answers",
                format_icon_colored(NerdFont::List, colors::BLUE)
            ),
            Self::GoBack => format!("{} Go Back", format_back_icon()),
            Self::UseDefault => format!(
                "{} Use Default",
                format_icon_colored(NerdFont::Check, colors::TEAL)
            ),
            Self::Abort => format!(
                "{} Abort",
                format_icon_colored(NerdFont::CrossCircle, colors::RED)
            ),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            Self::Resume => PreviewBuilder::new()
                .header(NerdFont::Play, "Resume")
                .text("Continue the current question flow.")
                .blank()
                .line(
                    colors::GREEN,
                    Some(NerdFont::Check),
                    "Keeps all current answers.",
                )
                .build(),
            Self::ReviewAnswers => review_answers_preview(),
            Self::GoBack => PreviewBuilder::new()
                .header(NerdFont::ArrowLeft, "Go Back")
                .text("Return to the previous question.")
                .blank()
                .line(
                    colors::PEACH,
                    Some(NerdFont::ArrowLeft),
                    "Re-answer the previous step.",
                )
                .build(),
            Self::UseDefault => PreviewBuilder::new()
                .header(NerdFont::Check, "Use Default")
                .text("Continue without answering; the default value will be applied.")
                .blank()
                .line(
                    colors::GREEN,
                    Some(NerdFont::Check),
                    "The wizard continues with the default answer.",
                )
                .build(),
            Self::Abort => abort_preview(),
        }
    }
}

#[derive(Clone)]
pub(super) enum FinalReviewAction {
    Complete,
    ReviewAnswers,
    AdvancedOptions,
    Abort,
}

#[derive(Clone)]
pub(super) struct FinalReviewOption {
    pub(super) action: FinalReviewAction,
    label: String,
    preview: FzfPreview,
}

impl FinalReviewOption {
    fn new(action: FinalReviewAction, label: impl Into<String>, preview: FzfPreview) -> Self {
        Self {
            action,
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
        match self.action {
            FinalReviewAction::Complete => "complete",
            FinalReviewAction::ReviewAnswers => "review_answers",
            FinalReviewAction::AdvancedOptions => "advanced_options",
            FinalReviewAction::Abort => "abort",
        }
        .to_string()
    }
}

#[derive(Clone)]
pub(super) enum ReviewItem {
    Continue,
    Question {
        index: usize,
        id: QuestionId,
        description: String,
        answer: String,
        is_sensitive: bool,
    },
}

impl FzfSelectable for ReviewItem {
    fn fzf_display_text(&self) -> String {
        match self {
            Self::Continue => format!(
                "{} Continue",
                format_icon_colored(NerdFont::ArrowRight, colors::GREEN)
            ),
            Self::Question {
                id,
                answer,
                is_sensitive,
                ..
            } => {
                let display_answer = display_answer(answer, *is_sensitive);
                let truncated = truncate_answer(&display_answer);
                format!(
                    "{} {id:?}: {truncated}",
                    format_icon_colored(NerdFont::Check, colors::TEAL)
                )
            }
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            Self::Continue => PreviewBuilder::new()
                .header(NerdFont::ArrowRight, "Continue")
                .text("Resume the wizard.")
                .blank()
                .line(
                    colors::GREEN,
                    Some(NerdFont::Check),
                    "All reviewed answers will be kept.",
                )
                .build(),
            Self::Question {
                id,
                description,
                answer,
                is_sensitive,
                ..
            } => {
                let answer = display_answer(answer, *is_sensitive);
                let mut builder =
                    PreviewBuilder::new().header(NerdFont::Question, &format!("{id:?}"));
                if !description.is_empty() {
                    builder = builder.subtext(description);
                }
                builder
                    .blank()
                    .field("Current Answer", &answer)
                    .blank()
                    .line(colors::TEAL, None, "Select to re-answer this question.")
                    .build()
            }
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            Self::Continue => "continue".to_string(),
            Self::Question { index, .. } => format!("question_{index}"),
        }
    }
}

#[derive(Clone)]
pub(super) enum AdvancedOption {
    Back,
    Question {
        index: usize,
        id: QuestionId,
        description: String,
        answer: Option<String>,
        is_sensitive: bool,
    },
}

impl AdvancedOption {
    pub(super) fn from_questions(
        questions: &[Box<dyn Question>],
        context: &InstallContext,
    ) -> Vec<Self> {
        let mut options = vec![Self::Back];
        options.extend(
            questions
                .iter()
                .enumerate()
                .filter(|(_, question)| question.is_optional() && question.should_ask(context))
                .map(|(index, question)| Self::Question {
                    index,
                    id: question.id(),
                    description: question.description().unwrap_or_default().to_string(),
                    answer: context.get_answer(&question.id()).cloned(),
                    is_sensitive: question.is_sensitive(),
                }),
        );
        options
    }
}

impl FzfSelectable for AdvancedOption {
    fn fzf_display_text(&self) -> String {
        match self {
            Self::Back => format!("{} Back", NerdFont::ArrowLeft),
            Self::Question {
                id,
                answer,
                is_sensitive,
                ..
            } => match answer {
                Some(answer) => format!(
                    "{} {id:?} (Current: {})",
                    NerdFont::Gear,
                    display_answer(answer, *is_sensitive)
                ),
                None => format!("{} {id:?}", NerdFont::Gear),
            },
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            Self::Back => PreviewBuilder::new()
                .header(NerdFont::ArrowLeft, "Back")
                .text("Return to the final review.")
                .build(),
            Self::Question {
                id,
                description,
                answer,
                is_sensitive,
                ..
            } => {
                let mut builder = PreviewBuilder::new().header(NerdFont::Gear, &format!("{id:?}"));
                if !description.is_empty() {
                    builder = builder.subtext(description);
                }
                if let Some(answer) = answer {
                    builder = builder
                        .blank()
                        .field("Current Answer", &display_answer(answer, *is_sensitive));
                }
                builder.build()
            }
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            Self::Back => "back".to_string(),
            Self::Question { index, .. } => format!("question_{index}"),
        }
    }
}

pub(super) fn final_review_options(
    flow: FlowKind,
    context: &InstallContext,
) -> Vec<FinalReviewOption> {
    match flow {
        FlowKind::Install => install_review_options(context),
        FlowKind::Setup => setup_review_options(context),
    }
}

fn install_review_options(context: &InstallContext) -> Vec<FinalReviewOption> {
    let summary = build_install_summary(context);
    vec![
        FinalReviewOption::new(
            FinalReviewAction::Complete,
            format!(
                "{} Install",
                format_icon_colored(NerdFont::Download, colors::GREEN)
            ),
            install_review_preview(&FinalReviewAction::Complete, &summary),
        ),
        FinalReviewOption::new(
            FinalReviewAction::ReviewAnswers,
            format!(
                "{} Review Answers",
                format_icon_colored(NerdFont::List, colors::BLUE)
            ),
            install_review_preview(&FinalReviewAction::ReviewAnswers, &summary),
        ),
        FinalReviewOption::new(
            FinalReviewAction::AdvancedOptions,
            format!(
                "{} Advanced Options",
                format_icon_colored(NerdFont::Sliders, colors::LAVENDER)
            ),
            install_review_preview(&FinalReviewAction::AdvancedOptions, &summary),
        ),
        FinalReviewOption::new(
            FinalReviewAction::Abort,
            format!(
                "{} Abort",
                format_icon_colored(NerdFont::CrossCircle, colors::RED)
            ),
            abort_preview(),
        ),
    ]
}

fn setup_review_options(context: &InstallContext) -> Vec<FinalReviewOption> {
    let summary = build_setup_summary(context);
    vec![
        FinalReviewOption::new(
            FinalReviewAction::Complete,
            format!(
                "{} Apply Setup",
                format_icon_colored(NerdFont::Download, colors::GREEN)
            ),
            PreviewBuilder::new()
                .header(NerdFont::Download, "Apply Setup")
                .text("Configure this system with instantOS.")
                .blank()
                .raw(&summary)
                .build(),
        ),
        FinalReviewOption::new(
            FinalReviewAction::ReviewAnswers,
            format!(
                "{} Review Answers",
                format_icon_colored(NerdFont::List, colors::BLUE)
            ),
            PreviewBuilder::new()
                .header(NerdFont::List, "Review Answers")
                .text("Browse and edit your previous responses.")
                .blank()
                .raw(&summary)
                .build(),
        ),
        FinalReviewOption::new(
            FinalReviewAction::Abort,
            format!(
                "{} Abort",
                format_icon_colored(NerdFont::CrossCircle, colors::RED)
            ),
            abort_preview(),
        ),
    ]
}

fn install_review_preview(action: &FinalReviewAction, summary: &InstallSummary) -> FzfPreview {
    match action {
        FinalReviewAction::Complete => {
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
        FinalReviewAction::ReviewAnswers => PreviewBuilder::new()
            .header(NerdFont::List, "Review Answers")
            .text("Browse and edit your previous responses.")
            .blank()
            .raw(&summary.text)
            .build(),
        FinalReviewAction::AdvancedOptions => PreviewBuilder::new()
            .header(NerdFont::Sliders, "Advanced Options")
            .text("Configure optional steps before installing.")
            .blank()
            .raw(&summary.text)
            .build(),
        FinalReviewAction::Abort => abort_preview(),
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

fn abort_preview() -> FzfPreview {
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

fn display_answer(answer: &str, is_sensitive: bool) -> String {
    if is_sensitive {
        "******".to_string()
    } else {
        answer.to_string()
    }
}

fn truncate_answer(answer: &str) -> String {
    const MAX_CHARS: usize = 50;
    const PREFIX_CHARS: usize = 47;
    if answer.chars().count() <= MAX_CHARS {
        return answer.to_string();
    }
    format!("{}…", answer.chars().take(PREFIX_CHARS).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::{display_answer, truncate_answer};

    #[test]
    fn sensitive_answers_are_masked() {
        assert_eq!(
            display_answer("correct horse battery staple", true),
            "******"
        );
    }

    #[test]
    fn long_unicode_answers_are_truncated_on_character_boundaries() {
        let answer = "界".repeat(60);
        let truncated = truncate_answer(&answer);

        assert_eq!(truncated.chars().count(), 48);
        assert!(truncated.ends_with('…'));
    }
}
