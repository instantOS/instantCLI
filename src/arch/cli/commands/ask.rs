use anyhow::Result;

use crate::arch::cli::DEFAULT_QUESTIONS_FILE;
use crate::arch::engine::{
    InstallContext, InstallSummary, QuestionEngine, QuestionId, SystemInfo, build_install_summary,
};
use crate::common::distro::is_live_iso;
use crate::menu_utils::{FzfPreview, FzfResult, FzfSelectable, FzfWrapper};
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

    fn fzf_key(&self) -> String {
        self.label.clone()
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
        .select(options)?;

    match selection {
        FzfResult::Selected(option) => Ok(Some(option.choice)),
        _ => Ok(None),
    }
}

enum ExistingAnswersOutcome {
    Proceed(Option<InstallContext>),
    Cancelled,
}

fn resolve_config_path(output_config: Option<std::path::PathBuf>) -> std::path::PathBuf {
    output_config.unwrap_or_else(|| std::path::PathBuf::from(DEFAULT_QUESTIONS_FILE))
}

fn ensure_internet(system_info: &SystemInfo) -> Result<bool> {
    if system_info.internet_connected {
        return Ok(true);
    }

    eprintln!("Error: No internet connection detected. Arch installation requires internet.");
    Ok(false)
}

fn install_live_iso_dependencies() -> Result<()> {
    if !is_live_iso() {
        return Ok(());
    }

    println!("Detected Arch Linux Live ISO environment.");

    let dependencies = &[
        &crate::common::deps::FZF,
        &crate::common::deps::GIT,
        &crate::common::deps::LIBGIT2,
        &crate::common::deps::GUM,
        &crate::common::deps::CFDISK,
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
        if let Err(e) = crate::arch::execution::pacman::install(&missing_packages, &executor) {
            eprintln!("Warning: Failed to install some packages: {}", e);
        } else {
            println!("Successfully installed {} packages", missing_packages.len());
        }
    }

    Ok(())
}

fn print_system_checks(system_info: &SystemInfo) {
    println!("System Checks:");
    println!("  Boot Mode: {}", system_info.boot_mode);
    println!("  Internet: {}", system_info.internet_connected);
    println!("  AMD CPU: {}", system_info.has_amd_cpu);
    println!("  Intel CPU: {}", system_info.has_intel_cpu);
    println!("  GPUs: {:?}", system_info.gpus);
    println!("  Virtual Machine: {:?}", system_info.vm_type);
    println!("  RAM: {:?} GB", system_info.total_ram_gb);
}

fn load_existing_context(
    config_path: &std::path::Path,
    system_info: &SystemInfo,
) -> Result<ExistingAnswersOutcome> {
    if !config_path.exists() {
        return Ok(ExistingAnswersOutcome::Proceed(None));
    }

    match InstallContext::load(config_path) {
        Ok(mut context) => {
            if context.answers.is_empty() {
                return Ok(ExistingAnswersOutcome::Proceed(None));
            }

            context.system_info = system_info.clone();
            let summary = build_install_summary(&context);
            let answers_count = context.answers.len();
            match prompt_existing_answers(&summary, config_path, answers_count)? {
                Some(ExistingAnswersChoice::UseExisting) => {
                    Ok(ExistingAnswersOutcome::Proceed(Some(context)))
                }
                Some(ExistingAnswersChoice::StartOver) => {
                    std::fs::remove_file(config_path)?;
                    Ok(ExistingAnswersOutcome::Proceed(None))
                }
                None => Ok(ExistingAnswersOutcome::Cancelled),
            }
        }
        Err(err) => {
            let _ = FzfWrapper::message(&format!(
                "Existing configuration could not be read and will be ignored:\n{}",
                err
            ));
            Ok(ExistingAnswersOutcome::Proceed(None))
        }
    }
}

fn build_question_engine(
    questions: Vec<Box<dyn crate::arch::engine::Question>>,
    system_info: SystemInfo,
    existing_context: Option<InstallContext>,
) -> QuestionEngine {
    let mut engine = QuestionEngine::new(questions);
    if let Some(context) = existing_context {
        engine.context = context;
    }
    engine.context.system_info = system_info;
    engine
}

fn print_completion_summary(context: &InstallContext) {
    println!("Installation configuration complete!");
    println!(
        "Hostname: {}",
        context
            .get_answer(&QuestionId::Hostname)
            .map_or("<not set>".to_string(), |v| v.clone())
    );
    println!(
        "Username: {}",
        context
            .get_answer(&QuestionId::Username)
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
    id: QuestionId,
    questions: Vec<Box<dyn crate::arch::engine::Question>>,
) -> Result<()> {
    // Ask a single question
    // Escalate if the question requires root (e.g. Disk)
    if matches!(id, QuestionId::Disk) {
        ensure_root()?;
    }

    let question = questions
        .into_iter()
        .find(|q| q.id() == id)
        .ok_or_else(|| anyhow::anyhow!("Question not found"))?;

    let engine = QuestionEngine::new(vec![question]);

    // Initialize data providers so questions that need data (like MirrorRegion) work
    engine.initialize_providers();

    // Run the engine with just this single question
    let context = engine.run().await?;

    if let Some(answer) = context.get_answer(&id) {
        println!("Answer: {}", answer);
    }
    Ok(())
}

async fn run_full_wizard(
    output_config: Option<std::path::PathBuf>,
    questions: Vec<Box<dyn crate::arch::engine::Question>>,
) -> Result<()> {
    // Installation requires root privileges
    ensure_root()?;

    println!("Starting Arch Linux installation wizard...");

    let config_path = resolve_config_path(output_config);

    // Perform system checks
    let system_info = SystemInfo::detect();

    if !ensure_internet(&system_info)? {
        return Ok(());
    }

    install_live_iso_dependencies()?;
    print_system_checks(&system_info);

    let existing_context = match load_existing_context(&config_path, &system_info)? {
        ExistingAnswersOutcome::Proceed(context) => context,
        ExistingAnswersOutcome::Cancelled => return Ok(()),
    };

    let engine = build_question_engine(questions, system_info, existing_context);

    // Initialize data providers
    engine.initialize_providers();

    let context = engine.run().await?;

    print_completion_summary(&context);
    save_config(&context, &config_path)?;

    Ok(())
}

/// Handle the Ask command - either ask a single question or run the full questionnaire
pub(super) async fn handle_ask_command(
    id: Option<crate::arch::engine::QuestionId>,
    output_config: Option<std::path::PathBuf>,
    questions: Vec<Box<dyn crate::arch::engine::Question>>,
) -> Result<()> {
    if let Some(id) = id {
        return run_single_question(id, questions).await;
    }

    run_full_wizard(output_config, questions).await
}
