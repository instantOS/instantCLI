use std::io::IsTerminal;

use anyhow::Result;

use crate::arch::config::DesktopEnvironment;
use crate::arch::engine::{
    EngineOutcome, FlowKind, InstallContext, Question, QuestionEngine, QuestionId,
};
use crate::arch::questions::{DesktopEnvironmentQuestion, DisplayManagerQuestion};
use crate::common::distro::is_live_iso;
use crate::settings::users::validate_username;

use super::super::utils::{detect_single_user, ensure_root};
use super::{AutologinDefault, autologin_question};

pub(super) async fn handle_setup_command(user: Option<String>, dry_run: bool) -> Result<()> {
    // Check if running on live CD
    if is_live_iso() {
        anyhow::bail!("This command cannot be run on a live CD/ISO.");
    }

    // Validate the explicit `--user` override up front: setup targets an
    // existing account, and an invalid name would otherwise only surface as a
    // `usermod`/`su` failure mid-flow. The same Unix name rules apply as in
    // `ins settings` and the installer's username question. `SUDO_USER` and
    // auto-detected users are intentionally not re-validated: they refer to
    // accounts that already exist.
    if let Some(user) = &user
        && let Err(error) = validate_username(user)
    {
        anyhow::bail!("Invalid --user '{user}': {error}");
    }

    // The setup wizard is interactive. Like `ins arch install`, relaunch inside
    // a terminal so fzf prompts are never started invisibly.
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        let current_exe = std::env::current_exe()?;
        let launcher = crate::common::terminal::TerminalLauncher::new(
            current_exe.to_string_lossy().to_string(),
        )
        .args(setup_relaunch_args(user.as_deref(), dry_run))
        .class("ins-setup")
        .title("instantOS Setup");
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
    let engine =
        QuestionEngine::for_flow(FlowKind::Setup, setup_questions())?.with_context(context);
    let EngineOutcome::Completed(context) = engine.run().await? else {
        return Ok(());
    };

    print_setup_configuration(&context);

    let executor = crate::arch::execution::CommandExecutor::new(dry_run, None);
    crate::arch::execution::setup::setup_instantos(&context, &executor, target_user).await
}

fn setup_relaunch_args(user: Option<&str>, dry_run: bool) -> Vec<String> {
    let mut args = vec!["arch".to_string(), "setup".to_string()];
    if let Some(user) = user {
        args.extend(["--user".to_string(), user.to_string()]);
    }
    if dry_run {
        args.push("--dry-run".to_string());
    }
    args
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
        Box::new(autologin_question(AutologinDefault::Disabled)),
    ]
}

#[cfg(test)]
mod tests {
    use super::{setup_questions, setup_relaunch_args};
    use crate::arch::engine::{FlowKind, QuestionEngine};

    #[test]
    fn relaunch_preserves_setup_options() {
        assert_eq!(
            setup_relaunch_args(Some("alice"), true),
            ["arch", "setup", "--user", "alice", "--dry-run"]
        );
    }

    #[test]
    fn setup_question_graph_is_valid() {
        QuestionEngine::for_flow(FlowKind::Setup, setup_questions()).unwrap();
    }
}
