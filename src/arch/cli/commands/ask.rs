use anyhow::{Context, Result, bail};

use crate::arch::cli::DEFAULT_QUESTIONS_FILE;
use crate::arch::engine::{
    InstallContext, InstallPlan, InstallSummary, StepId, SystemInfo, WizardEngine, WizardOutcome,
    build_install_summary,
};
use crate::common::distro::is_live_iso;
use crate::menu_utils::{FzfPreview, FzfSelectable, FzfWrapper};
use crate::ui::catppuccin::{colors, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::super::utils::ensure_root;

#[derive(Clone, Copy)]
enum ExistingAnswersChoice {
    UseExisting,
    StartOver,
}

#[derive(Clone)]
struct ExistingAnswersOption {
    choice: ExistingAnswersChoice,
    label: String,
    preview: FzfPreview,
}

impl ExistingAnswersOption {
    fn new(choice: ExistingAnswersChoice, label: String, preview: FzfPreview) -> Self {
        Self {
            choice,
            label,
            preview,
        }
    }
}

impl FzfSelectable for ExistingAnswersOption {
    fn fzf_display_text(&self) -> String {
        self.label.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview.clone()
    }
}

fn build_existing_answers_preview(
    summary: &InstallSummary,
    config_path: &std::path::Path,
    answers_count: usize,
) -> FzfPreview {
    let answers_label = if answers_count == 1 {
        "1 answer".to_string()
    } else {
        format!("{} answers", answers_count)
    };

    PreviewBuilder::new()
        .header(NerdFont::FileText, "Use Saved Answers")
        .text("Load the saved configuration and continue the wizard.")
        .blank()
        .field("File", &config_path.display().to_string())
        .field("Saved", &answers_label)
        .blank()
        .line(colors::TEAL, None, "Summary")
        .raw(&summary.text)
        .build()
}

fn build_start_over_preview(config_path: &std::path::Path) -> FzfPreview {
    PreviewBuilder::new()
        .header(NerdFont::Broom, "Start Fresh")
        .text("Clear saved answers and restart the wizard.")
        .blank()
        .line(
            colors::YELLOW,
            Some(NerdFont::Warning),
            "Deletes the existing configuration file.",
        )
        .field("File", &config_path.display().to_string())
        .build()
}

fn prompt_existing_answers(
    summary: &InstallSummary,
    config_path: &std::path::Path,
    answers_count: usize,
) -> Result<Option<ExistingAnswersChoice>> {
    let options = vec![
        ExistingAnswersOption::new(
            ExistingAnswersChoice::UseExisting,
            format!(
                "{} Use saved answers",
                format_icon_colored(NerdFont::Clipboard, colors::GREEN)
            ),
            build_existing_answers_preview(summary, config_path, answers_count),
        ),
        ExistingAnswersOption::new(
            ExistingAnswersChoice::StartOver,
            format!(
                "{} Start fresh (clear answers)",
                format_icon_colored(NerdFont::Broom, colors::YELLOW)
            ),
            build_start_over_preview(config_path),
        ),
    ];

    let selection = FzfWrapper::builder()
        .header("Existing configuration found")
        .prompt("Select")
        .responsive_layout()
        .items(options)
        .padded()
        .select_one()?;

    match selection {
        crate::menu_utils::DialogOutcome::Submitted(option) => Ok(Some(option.choice)),
        crate::menu_utils::DialogOutcome::Cancelled => Ok(None),
    }
}

pub(super) enum AskOutcome {
    Completed,
    Cancelled,
}

enum ExistingContextOutcome {
    Continue(Option<Box<InstallContext>>),
    Cancelled,
}

fn resolve_config_path(output_config: Option<std::path::PathBuf>) -> std::path::PathBuf {
    output_config.unwrap_or_else(|| std::path::PathBuf::from(DEFAULT_QUESTIONS_FILE))
}

fn ensure_internet(system_info: &SystemInfo, mode: crate::arch::offline::Mode) -> Result<()> {
    use crate::arch::offline::Mode;

    if mode != Mode::Online {
        println!("Offline bundle detected: proceeding without an internet connection.");
        return Ok(());
    }

    if system_info.internet_connected {
        return Ok(());
    }

    bail!("No internet connection detected. Arch installation requires internet.");
}

fn install_live_iso_dependencies() -> Result<()> {
    if !is_live_iso() {
        return Ok(());
    }

    println!("Detected Arch Linux Live ISO environment.");

    let dependencies = &[
        &crate::common::deps::FZF,
        &crate::common::deps::GIT,
        &crate::common::deps::GUM,
        &crate::common::deps::CFDISK,
        &crate::common::deps::BTRFS_PROGS,
        &crate::common::deps::NTFSPROGS,
        &crate::common::deps::NTFS_3G,
    ];

    // Collect all missing packages first
    let mut missing_packages = Vec::new();
    for dep in dependencies {
        if !dep.is_installed()
            && let Some(pkg) = dep
                .packages
                .iter()
                .find(|p| p.manager == crate::common::package::PackageManager::Pacman)
        {
            missing_packages.push(pkg.package_name);
            println!("Will install missing dependency: {}...", dep.name);
        }
    }

    // Install all missing packages in one pacman call
    if !missing_packages.is_empty() {
        println!("Installing {} missing packages...", missing_packages.len());

        let executor = crate::arch::execution::CommandExecutor::new(false, None);
        crate::arch::execution::pacman::install(&missing_packages, &executor)?;
        println!("Successfully installed {} packages", missing_packages.len());
    }

    Ok(())
}

fn print_system_checks(system_info: &SystemInfo, install_mode: crate::arch::offline::Mode) {
    println!("System Checks:");
    println!("  Boot Mode: {}", system_info.boot_mode);
    println!("  Internet: {}", system_info.internet_connected);
    println!(
        "  Install Source: {}",
        match install_mode {
            crate::arch::offline::Mode::Online => "network mirrors".to_owned(),
            crate::arch::offline::Mode::Opportunistic => {
                "offline bundle (network fallback enabled)".to_owned()
            }
            crate::arch::offline::Mode::Strict => "offline bundle (strict, no network)".to_owned(),
        }
    );
    println!("  AMD CPU: {}", system_info.has_amd_cpu);
    println!("  Intel CPU: {}", system_info.has_intel_cpu);
    println!("  GPUs: {:?}", system_info.gpus);
    println!("  Virtual Machine: {:?}", system_info.vm_type);
    println!("  RAM: {:?} GB", system_info.total_ram_gb);
}

fn load_existing_context(
    config_path: &std::path::Path,
    system_info: &SystemInfo,
) -> Result<ExistingContextOutcome> {
    if !config_path.exists() {
        return Ok(ExistingContextOutcome::Continue(None));
    }

    match InstallContext::load(config_path) {
        Ok(mut context) => {
            if !context.has_answers() {
                return Ok(ExistingContextOutcome::Continue(None));
            }

            context.system_info = system_info.clone();
            let summary = build_install_summary(&context);
            let answers_count = context.answer_count();
            match prompt_existing_answers(&summary, config_path, answers_count)? {
                Some(ExistingAnswersChoice::UseExisting) => {
                    Ok(ExistingContextOutcome::Continue(Some(Box::new(context))))
                }
                Some(ExistingAnswersChoice::StartOver) => {
                    std::fs::remove_file(config_path)?;
                    Ok(ExistingContextOutcome::Continue(None))
                }
                None => Ok(ExistingContextOutcome::Cancelled),
            }
        }
        Err(err) => {
            let _ = FzfWrapper::message(&format!(
                "Existing configuration could not be read and will be ignored:\n{}",
                err
            ));
            Ok(ExistingContextOutcome::Continue(None))
        }
    }
}

fn build_wizard_engine(
    steps: Vec<Box<dyn crate::arch::engine::WizardStep>>,
    system_info: SystemInfo,
    existing_context: Option<Box<InstallContext>>,
) -> Result<WizardEngine> {
    let mut context = existing_context.map_or_else(InstallContext::default, |context| *context);
    context.system_info = system_info;
    Ok(WizardEngine::new(steps)?.with_context(context))
}

fn print_completion_summary(context: &InstallContext) {
    println!("Installation configuration complete!");
    println!(
        "Hostname: {}",
        context
            .get_answer(&StepId::Hostname)
            .map_or("<not set>".to_string(), |v| v.clone())
    );
    println!(
        "Username: {}",
        context
            .get_answer(&StepId::Username)
            .map_or("<not set>".to_string(), |v| v.clone())
    );
}

fn save_config(context: &InstallContext, config_path: &std::path::Path) -> Result<()> {
    let toml_content = context.to_toml()?;

    // Ensure parent directory exists
    if let Some(parent) = config_path.parent()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent)?;
    }

    // Write to file
    std::fs::write(config_path, &toml_content)?;
    println!("\nConfiguration saved to: {}", config_path.display());
    Ok(())
}

async fn run_single_question(
    id: StepId,
    steps: Vec<Box<dyn crate::arch::engine::WizardStep>>,
) -> Result<AskOutcome> {
    // Ask a single question
    // Escalate if the question requires root (e.g. Disk)
    if matches!(id, StepId::Disk) {
        ensure_root()?;
    }

    let step = steps
        .into_iter()
        .find(|q| q.id() == id)
        .ok_or_else(|| anyhow::anyhow!("Wizard step not found"))?;

    let engine = WizardEngine::new(vec![step])?;

    let WizardOutcome::Completed(context) = engine.run().await? else {
        return Ok(AskOutcome::Cancelled);
    };

    if let Some(answer) = context.get_answer(&id) {
        println!("Answer: {}", answer);
    }
    Ok(AskOutcome::Completed)
}

async fn run_full_wizard(
    output_config: Option<std::path::PathBuf>,
    steps: Vec<Box<dyn crate::arch::engine::WizardStep>>,
) -> Result<AskOutcome> {
    // Installation requires root privileges
    ensure_root()?;

    println!("Starting Arch Linux installation wizard...");

    let config_path = resolve_config_path(output_config);

    // Perform system checks
    let system_info = SystemInfo::detect();

    let install_mode = crate::arch::offline::mode();
    crate::arch::offline::validate(install_mode, system_info.internet_connected)?;
    ensure_internet(&system_info, install_mode)?;
    install_live_iso_dependencies()?;
    print_system_checks(&system_info, install_mode);

    let existing_context = match load_existing_context(&config_path, &system_info)? {
        ExistingContextOutcome::Continue(context) => context,
        ExistingContextOutcome::Cancelled => return Ok(AskOutcome::Cancelled),
    };

    let engine = build_wizard_engine(steps, system_info, existing_context)?;

    let WizardOutcome::Completed(context) = engine.run().await? else {
        return Ok(AskOutcome::Cancelled);
    };

    // The interactive path and `arch exec` share the exact same final
    // conversion. Validate before saving for immediate feedback, then repeat
    // after loading because a saved file is an untrusted boundary.
    InstallPlan::try_from(context.as_ref())
        .context("The completed answers do not form a valid installation plan")?;

    print_completion_summary(&context);
    save_config(&context, &config_path)?;

    Ok(AskOutcome::Completed)
}

/// Handle the Ask command - either run a single step or the full wizard.
pub(super) async fn handle_ask_command(
    id: Option<crate::arch::engine::StepId>,
    output_config: Option<std::path::PathBuf>,
    steps: Vec<Box<dyn crate::arch::engine::WizardStep>>,
) -> Result<AskOutcome> {
    if let Some(id) = id {
        return run_single_question(id, steps).await;
    }

    run_full_wizard(output_config, steps).await
}

#[cfg(test)]
mod tests {
    use super::ensure_internet;
    use crate::arch::engine::SystemInfo;
    use crate::arch::offline::Mode;

    #[test]
    fn ensure_internet_errors_when_offline() {
        let system_info = SystemInfo {
            internet_connected: false,
            ..SystemInfo::default()
        };

        let error = ensure_internet(&system_info, Mode::Online).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("No internet connection detected"),
        );
    }

    #[test]
    fn ensure_internet_passes_when_online() {
        let system_info = SystemInfo {
            internet_connected: true,
            ..SystemInfo::default()
        };

        ensure_internet(&system_info, Mode::Online).unwrap();
    }

    #[test]
    fn ensure_internet_skips_the_requirement_when_offline() {
        let system_info = SystemInfo {
            internet_connected: false,
            ..SystemInfo::default()
        };

        ensure_internet(&system_info, Mode::Opportunistic).unwrap();
        ensure_internet(&system_info, Mode::Strict).unwrap();
    }
}
