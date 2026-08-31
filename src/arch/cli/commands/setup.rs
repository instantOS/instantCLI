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
    let questions: Vec<Box<dyn Question>> = vec![
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
            }),
        ),
    ];
    let mut engine = QuestionEngine::for_flow(FlowKind::Setup, questions);
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
