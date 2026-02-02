use anyhow::Result;

use crate::arch::cli::DEFAULT_QUESTIONS_FILE;
use crate::arch::engine::{InstallSummary, QuestionEngine, build_install_summary};
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

/// Handle the Ask command - either ask a single question or run the full questionnaire
pub(super) async fn handle_ask_command(
    id: Option<crate::arch::engine::QuestionId>,
    output_config: Option<std::path::PathBuf>,
    questions: Vec<Box<dyn crate::arch::engine::Question>>,
) -> Result<()> {
    if let Some(id) = id {
        // Ask a single question
        // Escalate if the question requires root (e.g. Disk)
        if matches!(id, crate::arch::engine::QuestionId::Disk) {
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
    } else {
        // Ask all questions (formerly Install logic)
        // Installation requires root privileges
        ensure_root()?;

        println!("Starting Arch Linux installation wizard...");

        let config_path =
            output_config.unwrap_or_else(|| std::path::PathBuf::from(DEFAULT_QUESTIONS_FILE));

        // Perform system checks
        let system_info = crate::arch::engine::SystemInfo::detect();

        if !system_info.internet_connected {
            eprintln!(
                "Error: No internet connection detected. Arch installation requires internet."
            );
            return Ok(());
        }

        // Check if running on live ISO and handle dependencies
        if is_live_iso() {
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
                if let Err(e) =
                    crate::arch::execution::pacman::install(&missing_packages, &executor)
                {
                    eprintln!("Warning: Failed to install some packages: {}", e);
                } else {
                    println!("Successfully installed {} packages", missing_packages.len());
                }
            }
        }

        println!("System Checks:");
        println!("  Boot Mode: {}", system_info.boot_mode);
        println!("  Internet: {}", system_info.internet_connected);
        println!("  AMD CPU: {}", system_info.has_amd_cpu);
        println!("  Intel CPU: {}", system_info.has_intel_cpu);
        println!("  GPUs: {:?}", system_info.gpus);
        println!("  Virtual Machine: {:?}", system_info.vm_type);
        println!("  RAM: {:?} GB", system_info.total_ram_gb);

        let mut existing_context: Option<crate::arch::engine::InstallContext> = None;
        if config_path.exists() {
            match crate::arch::engine::InstallContext::load(&config_path) {
                Ok(mut context) => {
                    if !context.answers.is_empty() {
                        context.system_info = system_info.clone();
                        let summary = build_install_summary(&context);
                        let answers_count = context.answers.len();
                        match prompt_existing_answers(&summary, &config_path, answers_count)? {
                            Some(ExistingAnswersChoice::UseExisting) => {
                                existing_context = Some(context);
                            }
                            Some(ExistingAnswersChoice::StartOver) => {
                                std::fs::remove_file(&config_path)?;
                            }
                            None => return Ok(()),
                        }
                    }
                }
                Err(err) => {
                    let _ = FzfWrapper::message(&format!(
                        "Existing configuration could not be read and will be ignored:\n{}",
                        err
                    ));
                }
            }
        }

        let mut engine = QuestionEngine::new(questions);
        if let Some(context) = existing_context {
            engine.context = context;
        }
        engine.context.system_info = system_info;

        // Initialize data providers
        engine.initialize_providers();

        let context = engine.run().await?;

        println!("Installation configuration complete!");
        println!(
            "Hostname: {}",
            context
                .get_answer(&crate::arch::engine::QuestionId::Hostname)
                .map_or("<not set>".to_string(), |v| v.clone())
        );
        println!(
            "Username: {}",
            context
                .get_answer(&crate::arch::engine::QuestionId::Username)
                .map_or("<not set>".to_string(), |v| v.clone())
        );

        let toml_content = context.to_toml()?;

        // Ensure parent directory exists
        if let Some(parent) = config_path.parent()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent)?;
        }

        // Write to file
        std::fs::write(&config_path, &toml_content)?;
        println!("\nConfiguration saved to: {}", config_path.display());

        Ok(())
    }
}
