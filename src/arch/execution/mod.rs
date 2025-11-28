use anyhow::{Context, Result};
use std::io::{BufRead, Write};
use std::path::PathBuf;

pub mod base;
pub mod bootloader;
pub mod config;
pub mod disk;
pub mod fstab;
pub mod paths;
pub mod post;
pub mod setup;
pub mod state;
pub mod step;

use self::state::InstallState;
use self::step::InstallStep;

pub fn is_chroot() -> bool {
    // A simple check is to compare the device/inode of / and /proc/1/root
    // If they are different, we are in a chroot.
    // If /proc is not mounted, this might fail, but in our context it should be.

    use std::os::unix::fs::MetadataExt;

    let root_meta = match std::fs::metadata("/") {
        Ok(m) => m,
        Err(_) => return false, // Assume not chroot if we can't stat /
    };

    let proc_root_meta = match std::fs::metadata("/proc/1/root") {
        Ok(m) => m,
        Err(_) => return false, // Assume not chroot if we can't stat /proc/1/root
    };

    root_meta.dev() != proc_root_meta.dev() || root_meta.ino() != proc_root_meta.ino()
}

pub struct CommandExecutor {
    pub dry_run: bool,
    pub log_file: Option<PathBuf>,
}

impl CommandExecutor {
    pub fn new(dry_run: bool, log_file: Option<PathBuf>) -> Self {
        Self { dry_run, log_file }
    }

    fn log_to_file(&self, message: &str) {
        if let Some(log_path) = &self.log_file {
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log_path)
            {
                let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
                let _ = writeln!(file, "[{}] {}", timestamp, message);
            }
        }
    }

    pub fn log(&self, message: &str) {
        self.log_to_file(message);
    }

    pub fn run(&self, command: &mut std::process::Command) -> anyhow::Result<()> {
        let program = command.get_program().to_string_lossy();
        let args: Vec<_> = command.get_args().map(|a| a.to_string_lossy()).collect();
        let cmd_str = format!("{} {}", program, args.join(" "));

        self.log_to_file(&format!("RUN: {}", cmd_str));

        if self.dry_run {
            self.print_dry_run(command, None);
            Ok(())
        } else {
            // Stream stdout/stderr to terminal AND log file
            command.stdout(std::process::Stdio::piped());
            command.stderr(std::process::Stdio::piped());

            let mut child = command.spawn()?;

            let stdout = child.stdout.take().expect("Failed to capture stdout");
            let stderr = child.stderr.take().expect("Failed to capture stderr");

            let stdout_handle = Self::spawn_logger(stdout, self.log_file.clone(), false);
            let stderr_handle = Self::spawn_logger(stderr, self.log_file.clone(), true);

            let status = child.wait()?;

            // Wait for threads to finish reading
            let _ = stdout_handle.join();
            let _ = stderr_handle.join();

            if !status.success() {
                self.log_to_file(&format!("FAILED: {}", cmd_str));
                anyhow::bail!("Command failed: {:?}", command);
            }
            Ok(())
        }
    }

    pub fn run_with_input(
        &self,
        command: &mut std::process::Command,
        input: &str,
    ) -> anyhow::Result<()> {
        let program = command.get_program().to_string_lossy();
        let args: Vec<_> = command.get_args().map(|a| a.to_string_lossy()).collect();
        let cmd_str = format!("{} {}", program, args.join(" "));

        self.log_to_file(&format!("RUN WITH INPUT: {}", cmd_str));
        // Don't log potentially sensitive input like passwords, but maybe log length?
        // For now let's just log that input was provided.
        self.log_to_file("(Input provided)");

        if self.dry_run {
            self.print_dry_run(command, Some(input));
            Ok(())
        } else {
            use std::io::Write;
            command.stdin(std::process::Stdio::piped());
            command.stdout(std::process::Stdio::piped());
            command.stderr(std::process::Stdio::piped());

            let mut child = command.spawn()?;

            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(input.as_bytes())?;
            }

            let stdout = child.stdout.take().expect("Failed to capture stdout");
            let stderr = child.stderr.take().expect("Failed to capture stderr");

            let stdout_handle = Self::spawn_logger(stdout, self.log_file.clone(), false);
            let stderr_handle = Self::spawn_logger(stderr, self.log_file.clone(), true);

            let status = child.wait()?;

            let _ = stdout_handle.join();
            let _ = stderr_handle.join();

            if !status.success() {
                self.log_to_file(&format!("FAILED: {}", cmd_str));
                anyhow::bail!("Command failed: {:?}", command);
            }
            Ok(())
        }
    }

    fn spawn_logger<R: std::io::Read + Send + 'static>(
        reader: R,
        log_file: Option<PathBuf>,
        is_stderr: bool,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let reader = std::io::BufReader::new(reader);
            for line in reader.lines() {
                if let Ok(l) = line {
                    if is_stderr {
                        eprintln!("{}", l);
                    } else {
                        println!("{}", l);
                    }

                    if let Some(path) = &log_file {
                        if let Ok(mut file) = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(path)
                        {
                            let prefix = if is_stderr { "STDERR: " } else { "" };
                            let _ = writeln!(file, "{}{}", prefix, l);
                        }
                    }
                }
            }
        })
    }

    pub fn run_with_output(
        &self,
        command: &mut std::process::Command,
    ) -> anyhow::Result<Option<std::process::Output>> {
        let program = command.get_program().to_string_lossy();
        let args: Vec<_> = command.get_args().map(|a| a.to_string_lossy()).collect();
        let cmd_str = format!("{} {}", program, args.join(" "));

        self.log_to_file(&format!("RUN WITH OUTPUT: {}", cmd_str));

        if self.dry_run {
            self.print_dry_run(command, None);
            Ok(None)
        } else {
            // Capture stdout/stderr
            command.stdout(std::process::Stdio::piped());
            // We don't necessarily want to capture stderr, maybe let it inherit?
            // But .output() captures both.
            let output = command.output()?;
            if !output.status.success() {
                self.log_to_file(&format!("FAILED: {}", cmd_str));
                let stderr = String::from_utf8_lossy(&output.stderr);
                self.log_to_file(&format!("STDERR: {}", stderr));
                anyhow::bail!("Command failed: {:?}", command);
            }
            Ok(Some(output))
        }
    }

    fn print_dry_run(&self, command: &std::process::Command, input: Option<&str>) {
        let program = command.get_program().to_string_lossy();
        let args: Vec<_> = command.get_args().map(|a| a.to_string_lossy()).collect();
        let cmd_str = format!("{} {}", program, args.join(" "));

        if let Some(input_str) = input {
            if input_str.contains('\n') {
                println!("[DRY RUN] --- BEGIN COMMAND ---");
                println!("> {}", cmd_str);
                println!("{}", input_str.trim());
                println!("[DRY RUN] --- END COMMAND ---");
            } else {
                println!(
                    "[DRY RUN] echo '{}' | {}",
                    input_str.replace('\n', "\\n"),
                    cmd_str
                );
            }
        } else {
            println!("[DRY RUN] {}", cmd_str);
        }
    }
}

pub async fn execute_installation(
    config_path: PathBuf,
    step: Option<String>,
    mut dry_run: bool,
    log_file: Option<PathBuf>,
) -> Result<()> {
    // Check for force dry-run file
    if std::path::Path::new(paths::DRY_RUN_FLAG).exists() {
        if !dry_run {
            println!(
                "Notice: {} exists, forcing dry-run mode.",
                paths::DRY_RUN_FLAG
            );
        }
        dry_run = true;
    }

    if dry_run {
        println!("*** DRY RUN MODE ENABLED - No changes will be made ***");
    }

    let executor = CommandExecutor::new(dry_run, log_file.clone());

    if let Some(_) = &log_file {
        executor.log(&format!(
            "Starting installation execution. Dry run: {}",
            dry_run
        ));
    }

    println!("Loading configuration from: {}", config_path.display());

    if !config_path.exists() {
        anyhow::bail!("Configuration file not found: {}", config_path.display());
    }

    let content = std::fs::read_to_string(&config_path)?;
    let context: crate::arch::engine::InstallContext = toml::from_str(&content)?;

    println!(
        "Loaded configuration for user: {:?}",
        context.get_answer(&crate::arch::engine::QuestionId::Username)
    );

    if let Some(step_name) = step {
        // Try to parse the step name
        // In a real implementation we might use clap's value parser if we exposed the enum directly in CLI,
        // but here we take a string to allow flexibility or partial matching if needed.
        // For now, let's just match against our known steps.
        let step_enum = match step_name.to_lowercase().as_str() {
            "disk" => InstallStep::Disk,
            "base" => InstallStep::Base,
            "fstab" => InstallStep::Fstab,
            "config" => InstallStep::Config,
            "bootloader" => InstallStep::Bootloader,
            "post" => InstallStep::Post,
            _ => anyhow::bail!("Unknown step: {}", step_name),
        };

        println!("Executing single step: {:?}", step_enum);
        execute_step(step_enum, &context, &executor, &config_path).await?;
    } else {
        println!("Executing all steps...");
        let steps = vec![
            InstallStep::Disk,
            InstallStep::Base,
            InstallStep::Fstab,
            InstallStep::Config,
            InstallStep::Bootloader,
            InstallStep::Post,
        ];

        for step in steps {
            execute_step(step, &context, &executor, &config_path).await?;
        }

        // Remove the config file from the chroot to prevent leaking sensitive data (passwords)
        if !dry_run {
            let chroot_config = paths::chroot_path(paths::CONFIG_FILE);
            if chroot_config.exists() {
                println!(
                    "Securing installation: Removing configuration file from target system..."
                );
                if let Err(e) = std::fs::remove_file(&chroot_config) {
                    println!("Warning: Failed to remove config file from chroot: {}", e);
                }
            }
        }
    }

    Ok(())
}

async fn execute_step(
    step: InstallStep,
    context: &crate::arch::engine::InstallContext,
    executor: &CommandExecutor,
    config_path: &std::path::Path,
) -> Result<()> {
    let in_chroot = is_chroot();
    let requires_chroot = step.requires_chroot();

    // Load state
    let mut state = InstallState::load().unwrap_or_else(|e| {
        println!("Warning: Failed to load install state: {}", e);
        InstallState::new()
    });

    // Check if already complete
    if state.is_complete(step) && !executor.dry_run {
        println!("Step {:?} is already complete. Skipping.", step);
        return Ok(());
    }

    // Check dependencies
    if let Err(missing) = state.check_dependencies(step) {
        if executor.dry_run {
            println!(
                "Warning: Missing dependencies for {:?}: {:?}. Proceeding (Dry Run).",
                step, missing
            );
        } else {
            anyhow::bail!("Missing dependencies for {:?}: {:?}", step, missing);
        }
    }

    if requires_chroot && !in_chroot && !executor.dry_run {
        println!(
            "Step {:?} requires chroot, setting up and entering...",
            step
        );
        setup_chroot(executor, config_path)?;

        // Construct command to run inside chroot
        // arch-chroot /mnt /usr/bin/ins arch exec <step> --config /etc/instant/install_config.toml
        // Note: we need to pass the step name as string.
        // We can convert enum to string via Debug or Display if implemented, or just match.
        // clap::ValueEnum implements Display/FromStr usually but let's be safe.
        let step_name = format!("{:?}", step).to_lowercase();

        let mut cmd = std::process::Command::new("arch-chroot");
        cmd.arg(paths::CHROOT_MOUNT)
            .arg("/usr/bin/ins")
            .arg("arch")
            .arg("exec")
            .arg(step_name)
            .arg("--questions-file")
            .arg(paths::CONFIG_FILE);

        if executor.dry_run {
            // Pass dry-run flag if we are dry-running
            cmd.arg("--dry-run");
        }

        executor.run(&mut cmd)?;

        // Collect logs from chroot
        if !executor.dry_run {
            let chroot_log = paths::chroot_path(paths::LOG_FILE);
            if chroot_log.exists() {
                if let Ok(content) = std::fs::read_to_string(&chroot_log) {
                    executor.log(&format!("--- BEGIN CHROOT LOG ({:?}) ---", step));
                    executor.log(&content);
                    executor.log(&format!("--- END CHROOT LOG ({:?}) ---", step));

                    // Remove the chroot log file to avoid duplication in subsequent steps
                    let _ = std::fs::remove_file(&chroot_log);
                }
            }
        }

        // Mark complete on host after successful chroot execution
        state.mark_complete(step);
        if let Err(e) = state.save() {
            println!("Warning: Failed to save install state on host: {}", e);
        }

        return Ok(());
    }

    if !requires_chroot && in_chroot {
        anyhow::bail!("Step {:?} should NOT be run inside chroot", step);
    }

    match step {
        InstallStep::Disk => disk::prepare_disk(context, executor)?,
        InstallStep::Base => base::install_base(context, executor).await?,
        InstallStep::Fstab => fstab::generate_fstab(context, executor)?,
        InstallStep::Config => {
            // setup_chroot is handled above if needed
            config::install_config(context, executor).await?
        }
        InstallStep::Bootloader => {
            // setup_chroot is handled above if needed
            bootloader::install_bootloader(context, executor).await?
        }
        InstallStep::Post => {
            // setup_chroot is handled above if needed
            post::install_post(context, executor).await?
        }
    }

    if !executor.dry_run {
        state.mark_complete(step);
        if let Err(e) = state.save() {
            println!("Warning: Failed to save install state: {}", e);
        } else if !in_chroot {
            // Sync state to chroot if it exists
            let chroot_state = paths::chroot_path(paths::STATE_FILE);
            if chroot_state.parent().map(|p| p.exists()).unwrap_or(false)
                && let Err(e) = std::fs::copy(paths::STATE_FILE, &chroot_state)
            {
                println!("Warning: Failed to sync state to chroot: {}", e);
            }
        }
    }

    Ok(())
}

fn setup_chroot(executor: &CommandExecutor, config_path: &std::path::Path) -> Result<()> {
    println!("Setting up chroot environment...");

    // Copy binary
    let current_exe = std::env::current_exe()?;
    let target_bin = paths::chroot_path("/usr/bin/ins");

    if executor.dry_run {
        println!("[DRY RUN] cp {:?} {:?}", current_exe, target_bin);
    } else {
        // We assume /mnt/usr/bin exists (created by base install)
        std::fs::copy(&current_exe, target_bin).context("Failed to copy binary to chroot")?;
    }

    // Copy config
    let target_config = paths::chroot_path(paths::CONFIG_FILE);
    if executor.dry_run {
        println!("[DRY RUN] cp {:?} {:?}", config_path, target_config);
    } else {
        // Ensure directory exists
        if let Some(parent) = target_config.parent()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent).context("Failed to create config dir in chroot")?;
        }
        std::fs::copy(config_path, target_config).context("Failed to copy config to chroot")?;
    }

    // Copy state file
    let state_file = paths::STATE_FILE;
    let target_state = paths::chroot_path(paths::STATE_FILE);
    if std::path::Path::new(state_file).exists() {
        if executor.dry_run {
            println!("[DRY RUN] cp {} {:?}", state_file, target_state);
        } else {
            // Ensure directory exists (should be same as config but good to be safe)
            if let Some(parent) = target_state.parent()
                && !parent.exists()
            {
                std::fs::create_dir_all(parent).context("Failed to create state dir in chroot")?;
            }
            std::fs::copy(state_file, target_state).context("Failed to copy state to chroot")?;
        }
    }

    Ok(())
}
