use anyhow::Result;

use crate::arch::cli::DEFAULT_QUESTIONS_FILE;
use crate::arch::engine::QuestionEngine;
use crate::common::distro::is_live_iso;

use super::super::utils::ensure_root;

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

        let mut engine = QuestionEngine::new(questions);
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

        let config_path =
            output_config.unwrap_or_else(|| std::path::PathBuf::from(DEFAULT_QUESTIONS_FILE));

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
