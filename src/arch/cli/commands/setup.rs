use std::io::IsTerminal;

use anyhow::Result;

use crate::arch::config::DesktopEnvironment;
use crate::arch::engine::{FlowKind, InstallContext, Question, QuestionEngine, QuestionId};
use crate::arch::questions::{BooleanQuestion, DesktopEnvironmentQuestion, DisplayManagerQuestion};
use crate::common::distro::is_live_iso;
use crate::ui::nerd_font::NerdFont;

use super::super::utils::{detect_single_user, ensure_root};

pub(super) async fn handle_setup_command(user: Option<String>, dry_run: bool) -> Result<()> {
    // Check if running on live CD
    if is_live_iso() {
        anyhow::bail!("This command cannot be run on a live CD/ISO.");
    }

    // The setup wizard is interactive. Like `ins arch install`, relaunch inside
    // a terminal so fzf prompts are never started invisibly.
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        let current_exe = std::env::current_exe()?;
        let mut launcher = crate::common::terminal::TerminalLauncher::new(
            current_exe.to_string_lossy().to_string(),
        )
        .arg("arch")
        .arg("setup")
        .class("ins-setup")
        .title("instantOS Setup");
        if let Some(user) = &user {
            launcher = launcher.arg("--user").arg(user);
        }
        launcher.launch()?;
        return Ok(());
    }

    if !dry_run {
        ensure_root()?;
    }

    // Try to infer user:
    // 1. Provided argument
    // 2. SUDO_USER env var
    // 3. Smart detection (single user in /home)
    let target_user = user
        .or_else(|| std::env::var("SUDO_USER").ok())
        .or_else(detect_single_user);

    // Create a context for setup by detecting existing system settings
    let context = crate::arch::engine::InstallContext::for_setup(target_user.clone());

    // Run the desktop-related questions from `ins arch install` as a small
    // setup wizard. The setup flow asks optional questions in the main flow
    // and reuses the engine's pause/review/back navigation.
    let mut engine = QuestionEngine::for_flow(FlowKind::Setup, setup_questions());
    engine.context = context;
    let context = engine.run().await?;

    print_setup_configuration(&context);

    let executor = crate::arch::execution::CommandExecutor::new(dry_run, None);
    crate::arch::execution::setup::setup_instantos(&context, &executor, target_user).await
}

fn print_setup_configuration(context: &InstallContext) {
    let answer = |id: QuestionId| -> String {
        context
            .get_answer(&id)
            .cloned()
            .unwrap_or_else(|| "<default>".to_string())
    };

    println!("\nSetup configuration:");
    println!(
        "  Desktop environment: {}",
        DesktopEnvironment::from_context(context).label()
    );
    if DesktopEnvironment::from_context(context).requires_display_manager() {
        println!("  Display manager: {}", answer(QuestionId::DisplayManager));
        println!("  Autologin: {}", answer(QuestionId::Autologin));
    }
    println!();
}

/// The desktop-related questions of the `ins arch install` wizard, asked
/// inline by the setup flow.
fn setup_questions() -> Vec<Box<dyn Question>> {
    vec![
        Box::new(DesktopEnvironmentQuestion),
        Box::new(DisplayManagerQuestion),
        Box::new(
            BooleanQuestion::new(
                QuestionId::Autologin,
                "Enable Display Manager Autologin?",
                NerdFont::User,
            )
            .optional()
            .should_ask(|context| {
                DesktopEnvironment::from_context(context).requires_display_manager()
            })
            .depends_on(vec![
                QuestionId::DesktopEnvironment,
                QuestionId::UseEncryption,
            ]),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dependencies that appear in a wizard's question list must precede the
    /// question that declares them, or predicates would silently read absent
    /// answers. Dependencies outside the list (e.g. pre-seeded contexts) are
    /// allowed.
    fn assert_dependencies_precede(questions: &[Box<dyn Question>]) {
        let ids: Vec<QuestionId> = questions.iter().map(|q| q.id()).collect();
        for (index, question) in questions.iter().enumerate() {
            for dependency in question.depends_on() {
                if let Some(dep_index) = ids.iter().position(|id| *id == dependency) {
                    assert!(
                        dep_index < index,
                        "{:?} declares a dependency on {:?}, which appears later in the question list",
                        question.id(),
                        dependency
                    );
                }
            }
        }
    }

    #[test]
    fn setup_question_dependencies_precede_their_questions() {
        assert_dependencies_precede(&setup_questions());
    }
}
