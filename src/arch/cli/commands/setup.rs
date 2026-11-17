use std::io::IsTerminal;

use anyhow::{Context, Result};

use crate::arch::arch_linux_variant::ArchLinuxVariant;
use crate::arch::config::DesktopEnvironment;
use crate::arch::engine::{FlowKind, InstallContext, StepId, WizardOutcome, WizardStep};
use crate::arch::host::HostProfile;
use crate::arch::questions::{DesktopEnvironmentQuestion, DisplayManagerQuestion};
use crate::settings::users::validate_username;

use super::super::utils::{detect_single_user, ensure_root};
use super::{AutologinDefault, autologin_question};

/// `ins arch setup` converts an existing Arch-family installation.
///
/// A live ISO has no installed system to convert, and a foreign distribution
/// has neither the package base nor the init layout the setup flow assumes.
fn ensure_setup_host(profile: HostProfile) -> Result<()> {
    match profile {
        HostProfile::RunningArch | HostProfile::RunningInstantOs | HostProfile::RunningArtix => {
            Ok(())
        }
        HostProfile::LiveIso => anyhow::bail!("This command cannot be run on a live CD/ISO."),
        HostProfile::ForeignDistro | HostProfile::Bootstrap => anyhow::bail!(
            "`ins arch setup` is only supported on Arch Linux, Artix Linux and instantOS. \
             This system is {}.",
            profile.label()
        ),
    }
}

pub(super) async fn handle_setup_command(user: Option<String>, dry_run: bool) -> Result<()> {
    ensure_setup_host(HostProfile::detect()?)?;

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
        let launcher = instantos_terminal::Launcher::new(current_exe.as_os_str())
            .args(setup_relaunch_args(user.as_deref(), dry_run))
            .app_id("ins-setup")
            .title("instantOS Setup");
        launcher.spawn_detached()?;
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

    let context = crate::arch::engine::InstallContext::for_setup(target_user.clone());
    let linux_variant =
        ArchLinuxVariant::detect().context("could not detect the installation variant")?;

    // Run the desktop-related questions from `ins arch install` as a small
    // setup wizard. The setup flow asks optional questions in the main flow
    // and reuses the engine's pause/review/back navigation.
    let steps = setup_questions();
    let engine =
        crate::arch::questions::wizard_engine(FlowKind::Setup, steps)?.with_context(context);
    let WizardOutcome::Completed(context) = engine.run().await? else {
        return Ok(());
    };

    print_setup_configuration(&context, linux_variant);

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

fn print_setup_configuration(context: &InstallContext, linux_variant: ArchLinuxVariant) {
    let answer = |id: StepId| -> String {
        context
            .get_answer(&id)
            .cloned()
            .unwrap_or_else(|| "<default>".to_string())
    };

    println!("\nSetup configuration:");
    println!(
        "  Arch Linux variant: {} ({})",
        linux_variant.label(),
        linux_variant.init_system().label()
    );
    println!(
        "  Desktop environment: {}",
        DesktopEnvironment::selected_or_default(context).label()
    );
    if DesktopEnvironment::selected_or_default(context).requires_display_manager() {
        println!("  Display manager: {}", answer(StepId::DisplayManager));
        println!("  Autologin: {}", answer(StepId::Autologin));
        println!("  Xorg server: {}", answer(StepId::UseXorg));
    }
    println!();
}

/// The desktop-related questions of the `ins arch install` wizard, asked
/// inline by the setup flow.
fn setup_questions() -> Vec<Box<dyn WizardStep>> {
    vec![
        Box::new(DesktopEnvironmentQuestion),
        Box::new(DisplayManagerQuestion),
        Box::new(autologin_question(AutologinDefault::Disabled)),
        Box::new(super::use_xorg_question()),
    ]
}

#[cfg(test)]
mod tests {
    use super::{ensure_setup_host, setup_questions, setup_relaunch_args};
    use crate::arch::engine::{FlowKind, WizardEngine};
    use crate::arch::host::HostProfile;

    #[test]
    fn relaunch_preserves_setup_options() {
        assert_eq!(
            setup_relaunch_args(Some("alice"), true),
            ["arch", "setup", "--user", "alice", "--dry-run"]
        );
    }

    #[test]
    fn setup_question_graph_is_valid() {
        WizardEngine::for_flow(FlowKind::Setup, setup_questions()).unwrap();
    }

    #[test]
    fn setup_accepts_every_running_arch_family_host() {
        for profile in [
            HostProfile::RunningArch,
            HostProfile::RunningInstantOs,
            HostProfile::RunningArtix,
        ] {
            assert!(
                ensure_setup_host(profile).is_ok(),
                "{} should support setup",
                profile.label()
            );
        }
    }

    #[test]
    fn setup_refuses_live_and_foreign_hosts() {
        let live = ensure_setup_host(HostProfile::LiveIso).unwrap_err();
        assert!(live.to_string().contains("live CD/ISO"), "{live}");

        assert!(ensure_setup_host(HostProfile::Bootstrap).is_err());

        let foreign = ensure_setup_host(HostProfile::ForeignDistro).unwrap_err();
        assert!(
            foreign
                .to_string()
                .contains("Arch Linux, Artix Linux and instantOS"),
            "the refusal names the supported systems: {foreign}"
        );
    }
}
