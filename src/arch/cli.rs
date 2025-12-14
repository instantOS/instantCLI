use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;

const DEFAULT_QUESTIONS_FILE: &str = "/etc/instant/questions.toml";

#[derive(Subcommand, Debug, Clone)]
pub enum DualbootCommands {
    /// Show information about existing operating systems and partitions
    Info,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ArchCommands {
    /// Start the Arch Linux installation wizard
    Install,
    /// List all available questions
    List,
    /// Ask a specific question
    Ask {
        /// The ID of the question to ask
        #[arg(value_enum)]
        id: Option<crate::arch::engine::QuestionId>,
        /// Optional path to save the configuration TOML file
        #[arg(short = 'o', long)]
        output_config: Option<std::path::PathBuf>,
    },
    /// Execute installation steps based on a questions file
    Exec {
        /// The step to execute (optional, defaults to all steps)
        #[arg(value_enum)]
        step: Option<String>,
        /// Path to the questions TOML file
        #[arg(short = 'f', long = "questions-file", default_value = DEFAULT_QUESTIONS_FILE)]
        questions_file: std::path::PathBuf,
        /// Run in dry-run mode (no changes will be made)
        #[arg(long)]
        dry_run: bool,
    },
    /// Show installation finished menu
    Finished,
    /// Setup instantOS on an existing Arch Linux installation
    Setup {
        /// Optional username to setup dotfiles for
        #[arg(short, long)]
        user: Option<String>,
        /// Run in dry-run mode
        #[arg(long)]
        dry_run: bool,
    },
    /// Upload installation logs to snips.sh
    UploadLogs {
        /// Path to the log file (optional, defaults to standard location)
        #[arg(short, long)]
        path: Option<std::path::PathBuf>,
    },
    /// Show system information in a pretty format
    Info,
    /// Dual boot detection and setup
    Dualboot {
        #[command(subcommand)]
        command: DualbootCommands,
    },
}

pub async fn handle_arch_command(command: ArchCommands, _debug: bool) -> Result<()> {
    use crate::arch::engine::QuestionEngine;
    use crate::arch::questions::{
        BooleanQuestion, DiskQuestion, DualBootPartitionQuestion, DualBootSizeQuestion,
        EncryptionPasswordQuestion, EspPartitionValidator, HostnameQuestion, KernelQuestion,
        KeymapQuestion, LocaleQuestion, MirrorRegionQuestion, PartitionSelectorQuestion,
        PartitioningMethodQuestion, PasswordQuestion, ResizeInstructionsQuestion,
        RunCfdiskQuestion, TimezoneQuestion, UsernameQuestion, VirtualBoxWarning,
        WeakPasswordWarning,
    };
    use crate::common::distro::{OperatingSystem, is_live_iso};

    let os = OperatingSystem::detect();
    if !os.is_arch_based() {
        eprintln!(
            "Warning: You appear to be running on {}, but this command is intended for Arch Linux.",
            os
        );
    }

    let questions: Vec<Box<dyn crate::arch::engine::Question>> = vec![
        Box::new(VirtualBoxWarning),
        Box::new(KeymapQuestion),
        Box::new(DiskQuestion),
        Box::new(PartitioningMethodQuestion),
        Box::new(RunCfdiskQuestion),
        Box::new(DualBootPartitionQuestion),
        Box::new(DualBootSizeQuestion),
        Box::new(ResizeInstructionsQuestion),
        Box::new(PartitionSelectorQuestion::new(
            crate::arch::engine::QuestionId::RootPartition,
            "Select Root Partition",
            crate::ui::nerd_font::NerdFont::HardDrive,
            None,
        )),
        Box::new(PartitionSelectorQuestion::new(
            crate::arch::engine::QuestionId::BootPartition,
            "Select Boot/EFI Partition",
            crate::ui::nerd_font::NerdFont::Folder,
            Some(Box::new(EspPartitionValidator)),
        )),
        Box::new(
            PartitionSelectorQuestion::new(
                crate::arch::engine::QuestionId::SwapPartition,
                "Select Swap Partition",
                crate::ui::nerd_font::NerdFont::File,
                None,
            )
            .optional(),
        ),
        Box::new(
            PartitionSelectorQuestion::new(
                crate::arch::engine::QuestionId::HomePartition,
                "Select Home Partition",
                crate::ui::nerd_font::NerdFont::Home,
                None,
            )
            .optional(),
        ),
        Box::new(HostnameQuestion),
        Box::new(UsernameQuestion),
        Box::new(PasswordQuestion),
        Box::new(
            BooleanQuestion::new(
                crate::arch::engine::QuestionId::UseEncryption,
                "Encrypt the installation disk?",
                crate::ui::nerd_font::NerdFont::Lock,
            )
            .dynamic_default(|context| {
                // Encryption features are only available for automatic partitioning
                // If manual partitioning is selected, encryption is not supported
                context
                    .get_answer(&crate::arch::engine::QuestionId::PartitioningMethod)
                    .map(|method| !method.contains("Manual"))
                    .unwrap_or(false)
            })
            .should_ask(|context| {
                // Only ask about encryption if automatic partitioning is selected
                context
                    .get_answer(&crate::arch::engine::QuestionId::PartitioningMethod)
                    .map(|method| !method.contains("Manual"))
                    .unwrap_or(true) // Default to true if partitioning method not yet answered
            }),
        ),
        Box::new(EncryptionPasswordQuestion),
        Box::new(WeakPasswordWarning),
        Box::new(MirrorRegionQuestion),
        Box::new(TimezoneQuestion),
        Box::new(LocaleQuestion),
        Box::new(KernelQuestion),
        Box::new(
            BooleanQuestion::new(
                crate::arch::engine::QuestionId::UsePlymouth,
                "Enable Plymouth boot splash screen?",
                crate::ui::nerd_font::NerdFont::Monitor,
            )
            .optional()
            .default_yes(),
        ),
        Box::new(
            BooleanQuestion::new(
                crate::arch::engine::QuestionId::Autologin,
                "Enable LightDM Autologin?",
                crate::ui::nerd_font::NerdFont::User,
            )
            .optional()
            .dynamic_default(|context| {
                context.get_answer_bool(crate::arch::engine::QuestionId::UseEncryption)
            }),
        ),
        Box::new(
            BooleanQuestion::new(
                crate::arch::engine::QuestionId::LogUpload,
                "Upload installation logs to snips.sh?",
                crate::ui::nerd_font::NerdFont::Debug,
            )
            .optional()
            .default_yes(),
        ),
        Box::new(
            BooleanQuestion::new(
                crate::arch::engine::QuestionId::MinimalMode,
                "Enable Minimal Mode (Vanilla Arch Install)?",
                crate::ui::nerd_font::NerdFont::Package,
            )
            .optional(),
        ),
    ];

    match command {
        ArchCommands::List => {
            println!("Available questions:");
            for question in questions {
                println!("- {:?}", question.id());
            }
            Ok(())
        }
        ArchCommands::Ask { id, output_config } => {
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
                            && let Some(pkg) = dep.packages.iter().find(|p| {
                                p.manager == crate::common::package::PackageManager::Pacman
                            })
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

                let config_path = output_config
                    .unwrap_or_else(|| std::path::PathBuf::from(DEFAULT_QUESTIONS_FILE));

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
        ArchCommands::Install => {
            // Check architecture
            let system_info = crate::arch::engine::SystemInfo::detect();

            // Check distro
            if !system_info.distro.contains("Arch") && !system_info.distro.contains("instantOS") {
                eprintln!(
                    "{} {}",
                    "Error:".red().bold(),
                    format!(
                        "Arch Linux installation is only supported on Arch Linux or instantOS. Detected distro: {}",
                        system_info.distro
                    ).red()
                );
                return Ok(());
            }

            if system_info.architecture != "x86_64" {
                eprintln!(
                    "{} {}",
                    "Error:".red().bold(),
                    format!(
                        "Arch Linux installation is only supported on x86_64 architecture. Detected architecture: {}",
                        system_info.architecture
                    ).red()
                );
                return Ok(());
            }

            // Mark start time
            let mut state = crate::arch::execution::state::InstallState::load()?;
            state.mark_start();
            state.save()?;

            // 1. Ask questions
            Box::pin(handle_arch_command(
                ArchCommands::Ask {
                    id: None,
                    output_config: None,
                },
                _debug,
            ))
            .await?;

            // 2. Execute
            let exec_result = Box::pin(handle_arch_command(
                ArchCommands::Exec {
                    step: None,
                    questions_file: std::path::PathBuf::from(DEFAULT_QUESTIONS_FILE),
                    dry_run: false,
                },
                _debug,
            ))
            .await;

            if let Err(_) = exec_result {
                // Try to upload logs if forced or requested
                if let Ok(context) =
                    crate::arch::engine::InstallContext::load(DEFAULT_QUESTIONS_FILE)
                {
                    crate::arch::logging::process_log_upload(&context);
                } else if std::path::Path::new("/etc/instantos/uploadlogs").exists() {
                    println!(
                        "Uploading installation logs (forced by /etc/instantos/uploadlogs)..."
                    );
                    let log_path =
                        std::path::PathBuf::from(crate::arch::execution::paths::LOG_FILE);
                    if let Err(e) = crate::arch::logging::upload_logs(&log_path) {
                        eprintln!("Failed to upload logs: {}", e);
                    }
                }
            }

            exec_result?;

            // 3. Finished
            Box::pin(handle_arch_command(ArchCommands::Finished, _debug)).await?;

            Ok(())
        }
        ArchCommands::Exec {
            step,
            questions_file,
            dry_run,
        } => {
            if !dry_run {
                ensure_root()?;
            }

            let log_file = if !dry_run {
                let path = std::path::PathBuf::from(crate::arch::execution::paths::LOG_FILE);
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                Some(path)
            } else {
                None
            };

            crate::arch::execution::execute_installation(questions_file, step, dry_run, log_file)
                .await
        }
        ArchCommands::UploadLogs { path } => {
            let log_path = path.unwrap_or_else(|| {
                std::path::PathBuf::from(crate::arch::execution::paths::LOG_FILE)
            });
            println!("Uploading logs from: {}", log_path.display());
            match crate::arch::logging::upload_logs(&log_path) {
                Ok(url) => println!("Logs uploaded successfully: {}", url.green().bold()),
                Err(e) => eprintln!("Failed to upload logs: {}", e),
            }
            Ok(())
        }
        ArchCommands::Info => {
            let info = crate::arch::engine::SystemInfo::detect();
            print_system_info(&info);
            Ok(())
        }
        ArchCommands::Dualboot { command } => match command {
            DualbootCommands::Info => {
                use crate::arch::dualboot::{check_all_disks_feasibility, display_disks};
                use crate::ui::nerd_font::NerdFont;

                println!();
                println!(
                    "  {} {}",
                    NerdFont::HardDrive.to_string().bright_cyan(),
                    "Dual Boot Detection".bright_white().bold()
                );
                println!("  {}", "─".repeat(50).bright_black());
                println!();

                match check_all_disks_feasibility() {
                    Ok((disks, feasibility_results)) => {
                        let mut any_feasible = false;

                        // Show feasibility summary first
                        println!("  {}", "Feasibility Summary:".bold());
                        for (disk, feasibility) in &feasibility_results {
                            if feasibility.feasible {
                                any_feasible = true;
                                // Show different message depending on whether feasibility
                                // comes from resizable partitions or unpartitioned space
                                if feasibility.feasible_partitions.is_empty() {
                                    // Feasible due to unpartitioned space
                                    println!(
                                        "    {} {} - {}",
                                        NerdFont::Check.to_string().green().bold(),
                                        disk.bright_white(),
                                        "FEASIBLE".green().bold(),
                                    );
                                    if let Some(reason) = &feasibility.reason {
                                        println!(
                                            "      {} {}",
                                            NerdFont::ArrowPointer.to_string().dimmed(),
                                            reason.green()
                                        );
                                    }
                                } else {
                                    // Feasible due to resizable partitions
                                    println!(
                                        "    {} {} - {} {}",
                                        NerdFont::Check.to_string().green().bold(),
                                        disk.bright_white(),
                                        "FEASIBLE".green().bold(),
                                        format!(
                                            "({} partition(s) can be resized)",
                                            feasibility.feasible_partitions.len()
                                        )
                                        .dimmed()
                                    );
                                    for part in &feasibility.feasible_partitions {
                                        println!(
                                            "      {} {}",
                                            NerdFont::ArrowPointer.to_string().dimmed(),
                                            part.cyan()
                                        );
                                    }
                                }
                            } else {
                                println!(
                                    "    {} {} - {}",
                                    NerdFont::Cross.to_string().red().bold(),
                                    disk.bright_white(),
                                    "NOT FEASIBLE".red().bold()
                                );
                                if let Some(reason) = &feasibility.reason {
                                    println!(
                                        "      {} {}",
                                        NerdFont::ArrowPointer.to_string().dimmed(),
                                        reason.dimmed()
                                    );
                                }
                            }
                        }

                        println!();

                        if any_feasible {
                            println!(
                                "  {} {}",
                                NerdFont::Check.to_string().green().bold(),
                                "Dual boot is POSSIBLE on this system".green().bold()
                            );
                            println!(
                                "  {} The installer will offer dual boot options",
                                NerdFont::ArrowPointer.to_string().dimmed()
                            );
                        } else {
                            println!(
                                "  {} {}",
                                NerdFont::Cross.to_string().red().bold(),
                                "Dual boot is NOT POSSIBLE on this system".red().bold()
                            );
                            println!(
                                "  {} The installer will NOT offer dual boot options",
                                NerdFont::ArrowPointer.to_string().dimmed()
                            );
                        }

                        println!();
                        println!("  {}", "Detailed Disk Information:".bold());
                        println!("  {}", "─".repeat(50).bright_black());
                        println!();

                        // Show detailed disk information
                        if disks.is_empty() {
                            println!(
                                "  {} No disks detected. Are you running as root?",
                                NerdFont::Warning.to_string().yellow()
                            );
                        } else {
                            display_disks(&disks);
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "  {} Failed to check feasibility: {}",
                            NerdFont::Cross.to_string().red(),
                            e
                        );
                        eprintln!(
                            "  {} Try running with sudo",
                            NerdFont::ArrowPointer.to_string().dimmed()
                        );
                    }
                }

                Ok(())
            }
        },

        ArchCommands::Finished => {
            use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper};
            use crate::ui::nerd_font::NerdFont;

            #[derive(Clone)]
            enum FinishedMenuOption {
                Reboot,
                Shutdown,
                Continue,
            }

            impl FzfSelectable for FinishedMenuOption {
                fn fzf_display_text(&self) -> String {
                    match self {
                        FinishedMenuOption::Reboot => format!("{} Reboot", NerdFont::Reboot),
                        FinishedMenuOption::Shutdown => format!("{} Shutdown", NerdFont::PowerOff),
                        FinishedMenuOption::Continue => {
                            format!("{} Continue in Live Session", NerdFont::Continue)
                        }
                    }
                }
            }

            let state = crate::arch::execution::state::InstallState::load()?;

            // Check if we should upload logs
            if let Ok(context) = crate::arch::engine::InstallContext::load(DEFAULT_QUESTIONS_FILE) {
                crate::arch::logging::process_log_upload(&context);
            }

            println!("\n{}", "Installation Finished!".green().bold());

            if let Some(start_time) = state.start_time {
                let duration = chrono::Utc::now() - start_time;
                let hours = duration.num_hours();
                let minutes = duration.num_minutes() % 60;
                let seconds = duration.num_seconds() % 60;
                println!("Duration: {:02}:{:02}:{:02}", hours, minutes, seconds);
            }

            // Calculate storage used (approximate)
            if let Ok(output) = std::process::Command::new("df")
                .arg("-h")
                .arg("/mnt")
                .output()
            {
                let output_str = String::from_utf8_lossy(&output.stdout);
                if let Some(line) = output_str.lines().nth(1) {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 3 {
                        println!("Storage Used: {}", parts[2]);
                    }
                }
            }

            println!();

            let options = vec![
                FinishedMenuOption::Reboot,
                FinishedMenuOption::Shutdown,
                FinishedMenuOption::Continue,
            ];

            let result = FzfWrapper::builder()
                .header("Installation complete. What would you like to do?")
                .select(options)?;

            match result {
                FzfResult::Selected(option) => match option {
                    FinishedMenuOption::Reboot => {
                        println!("Rebooting...");
                        std::process::Command::new("reboot").spawn()?;
                    }
                    FinishedMenuOption::Shutdown => {
                        println!("Shutting down...");
                        std::process::Command::new("poweroff").spawn()?;
                    }
                    FinishedMenuOption::Continue => {
                        println!("Exiting to live session...");
                    }
                },
                _ => println!("Exiting..."),
            }

            Ok(())
        }
        ArchCommands::Setup { user, dry_run } => {
            // Check if running on live CD
            if is_live_iso() {
                anyhow::bail!("This command cannot be run on a live CD/ISO.");
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

            let executor = crate::arch::execution::CommandExecutor::new(dry_run, None);
            crate::arch::execution::setup::setup_instantos(&context, &executor, target_user).await
        }
    }
}

fn detect_single_user() -> Option<String> {
    let home = std::path::Path::new("/home");
    if !home.exists() {
        return None;
    }

    let entries = match std::fs::read_dir(home) {
        Ok(e) => e,
        Err(_) => return None,
    };

    let mut users = Vec::new();
    for entry in entries.flatten() {
        if let Ok(file_type) = entry.file_type()
            && file_type.is_dir()
            && let Ok(name) = entry.file_name().into_string()
            && name != "lost+found"
        {
            users.push(name);
        }
    }

    if users.len() == 1 {
        Some(users[0].clone())
    } else {
        None
    }
}

fn ensure_root() -> Result<()> {
    if let sudo::RunningAs::User = sudo::check() {
        sudo::with_env(&["RUST_BACKTRACE", "RUST_LOG"])
            .map_err(|e| anyhow::anyhow!("Failed to escalate privileges: {}", e))?;
    }
    Ok(())
}

fn print_system_info(info: &crate::arch::engine::SystemInfo) {
    use crate::ui::nerd_font::NerdFont;
    use colored::*;

    let print_row = |icon: ColoredString, label: &str, value: &dyn std::fmt::Display| {
        println!("  {}   {:<20} {}", icon, label, value);
    };

    println!();
    println!(
        "  {} {}",
        NerdFont::Desktop.to_string().bright_cyan(),
        "System Information".bright_white().bold()
    );
    println!("  {}", "─".repeat(50).bright_black());

    // Distro
    print_row(
        NerdFont::Terminal.to_string().bright_cyan(),
        "Distro:",
        &info.distro.bright_cyan(),
    );

    // Boot Mode
    let boot_mode_str = match info.boot_mode {
        crate::arch::engine::BootMode::UEFI64 => "UEFI 64-bit",
        crate::arch::engine::BootMode::UEFI32 => "UEFI 32-bit",
        crate::arch::engine::BootMode::BIOS => "BIOS",
    };
    print_row(
        NerdFont::PowerOff.to_string().bright_green(),
        "Boot Mode:",
        &boot_mode_str.bright_green(),
    );

    // Architecture
    print_row(
        NerdFont::Cpu.to_string().bright_magenta(),
        "Architecture:",
        &info.architecture.bright_magenta(),
    );

    // CPU
    if info.has_intel_cpu {
        print_row(
            NerdFont::Cpu.to_string().bright_blue(),
            "CPU:",
            &"Intel".bright_blue(),
        );
    } else if info.has_amd_cpu {
        print_row(
            NerdFont::Cpu.to_string().bright_red(),
            "CPU:",
            &"AMD".bright_red(),
        );
    }

    // GPUs
    if !info.gpus.is_empty() {
        let gpu_strs: Vec<String> = info.gpus.iter().map(|gpu| gpu.to_string()).collect();
        let gpu_str = gpu_strs.join(", ");

        let colored_gpu_str = if info.gpus.len() == 1 {
            info.gpus[0].to_colored_string()
        } else {
            gpu_str.normal()
        };

        print_row(
            NerdFont::Gpu.to_string().bright_cyan(),
            "GPU:",
            &colored_gpu_str,
        );
    }

    // Virtualization
    if let Some(vm_type) = &info.vm_type {
        println!(
            "  {}   {:<20} {} ({})",
            NerdFont::Server.to_string().bright_yellow(),
            "Virtualization:",
            vm_type.bright_yellow(),
            "Virtual Machine".bright_black()
        );
    } else {
        print_row(
            NerdFont::Server.to_string().bright_green(),
            "Virtualization:",
            &"Bare Metal".bright_green(),
        );
    }

    // Internet
    if info.internet_connected {
        print_row(
            NerdFont::Globe.to_string().bright_green(),
            "Internet:",
            &"Connected".bright_green(),
        );
    } else {
        print_row(
            NerdFont::Globe.to_string().bright_red(),
            "Internet:",
            &"Disconnected".bright_red(),
        );
    }

    println!("  {}", "─".repeat(50).bright_black());
    println!();
}
