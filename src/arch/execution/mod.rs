use anyhow::{Context, Result};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::ui::nerd_font::NerdFont;

pub mod base;
pub mod bootloader;
pub mod config;
pub mod disk;
pub mod fstab;
pub mod packages;
pub mod pacman;
pub mod paths;
pub mod post;
pub mod setup;
pub mod state;
pub mod step;
pub mod upload_state;

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

/// Abstraction over command execution, used to make the execution layer testable.
pub trait CommandRunner {
    fn dry_run(&self) -> bool;
    fn run(&self, command: &mut std::process::Command) -> anyhow::Result<()>;
    fn run_with_input(
        &self,
        command: &mut std::process::Command,
        input: &str,
    ) -> anyhow::Result<()>;
    fn run_with_output(
        &self,
        command: &mut std::process::Command,
    ) -> anyhow::Result<Option<std::process::Output>>;

    /// Runs a command whose failure must not abort the installation.
    ///
    /// This is how the execution layer expresses a best-effort nicety: a step
    /// the installed system does not depend on (e.g. fetching a random
    /// wallpaper from a third-party service). The command runs normally with
    /// output streamed; on failure a warning is printed, the failure is
    /// logged, and execution continues.
    ///
    /// Returns `true` when the command succeeded, so callers can offer a
    /// follow-up hint. Use [`CommandRunner::run`] for anything the
    /// installation depends on.
    fn run_best_effort(&self, command: &mut std::process::Command, description: &str) -> bool {
        if self.run(command).is_ok() {
            return true;
        }
        println!(
            "{} {} failed; continuing without it.",
            NerdFont::Warning,
            description
        );
        self.log(&format!("BEST-EFFORT FAILED: {}", description));
        false
    }

    fn log(&self, message: &str);
}

pub struct CommandExecutor {
    pub dry_run: bool,
    pub log_file: Option<PathBuf>,
}

/// Commands that ran at least this long are announced on the terminal, not
/// just in the log file: they are where an installation's time goes, and a
/// user staring at a quiet terminal deserves to see the big pieces move.
const SLOW_COMMAND_THRESHOLD: Duration = Duration::from_secs(10);

impl CommandExecutor {
    pub fn new(dry_run: bool, log_file: Option<PathBuf>) -> Self {
        Self { dry_run, log_file }
    }

    /// Record how long a spawned command took. The log file gets every
    /// command's duration (this is what makes `install.log` a profiler);
    /// the terminal only hears about the slow ones.
    fn log_command_duration(&self, cmd_str: &str, started: Instant, succeeded: bool) {
        let elapsed = started.elapsed();
        let outcome = if succeeded { "DONE" } else { "FAILED" };
        self.log_to_file(&format!(
            "{} ({:.1}s): {}",
            outcome,
            elapsed.as_secs_f64(),
            cmd_str
        ));
        if elapsed >= SLOW_COMMAND_THRESHOLD {
            let verb = if succeeded { "finished" } else { "failed" };
            println!("'{}' {} in {:.0}s", cmd_str, verb, elapsed.as_secs_f64());
        }
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

        let started = Instant::now();
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

            let succeeded = status.success();
            self.log_command_duration(&cmd_str, started, succeeded);
            if !succeeded {
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

        let started = Instant::now();
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

            let succeeded = status.success();
            self.log_command_duration(&cmd_str, started, succeeded);
            if !succeeded {
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
            for l in reader.lines().map_while(Result::ok) {
                if is_stderr {
                    eprintln!("{}", l);
                } else {
                    println!("{}", l);
                }

                if let Some(path) = &log_file
                    && let Ok(mut file) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                {
                    let prefix = if is_stderr { "STDERR: " } else { "" };
                    let _ = writeln!(file, "{}{}", prefix, l);
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

        let started = Instant::now();
        if self.dry_run {
            self.print_dry_run(command, None);
            Ok(None)
        } else {
            // Capture stdout/stderr
            command.stdout(std::process::Stdio::piped());
            // We don't necessarily want to capture stderr, maybe let it inherit?
            // But .output() captures both.
            let output = command.output()?;
            let succeeded = output.status.success();
            self.log_command_duration(&cmd_str, started, succeeded);
            if !succeeded {
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

impl CommandRunner for CommandExecutor {
    fn dry_run(&self) -> bool {
        self.dry_run
    }

    fn run(&self, command: &mut std::process::Command) -> anyhow::Result<()> {
        CommandExecutor::run(self, command)
    }

    fn run_with_input(
        &self,
        command: &mut std::process::Command,
        input: &str,
    ) -> anyhow::Result<()> {
        CommandExecutor::run_with_input(self, command, input)
    }

    fn run_with_output(
        &self,
        command: &mut std::process::Command,
    ) -> anyhow::Result<Option<std::process::Output>> {
        CommandExecutor::run_with_output(self, command)
    }

    fn log(&self, message: &str) {
        CommandExecutor::log(self, message);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionOutcome {
    Completed,
    AlreadyInstalled,
}

pub async fn execute_installation(
    steps: &[Box<dyn crate::arch::engine::WizardStep>],
    config_path: PathBuf,
    step: Option<String>,
    mut dry_run: bool,
    log_file: Option<PathBuf>,
) -> Result<ExecutionOutcome> {
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

    // Strict mode demands a bundle; fail before touching the disk.
    let install_mode = crate::arch::offline::mode();
    crate::arch::offline::validate(install_mode)?;

    // Increase cowspace if in live ISO
    if crate::common::distro::is_live_iso()
        && !dry_run
        && let Err(e) = crate::common::distro::increase_cowspace()
    {
        println!("Warning: Failed to increase cowspace: {}", e);
    }

    let executor = CommandExecutor::new(dry_run, log_file.clone());

    if log_file.is_some() {
        executor.log(&format!(
            "Starting installation execution. Dry run: {}",
            dry_run
        ));
    }

    let installation_started = Instant::now();
    // Captured before `step` is consumed by the single-step branch below.
    let full_installation = step.is_none();

    println!("Loading configuration from: {}", config_path.display());

    if !config_path.exists() {
        anyhow::bail!("Configuration file not found: {}", config_path.display());
    }

    let content = std::fs::read_to_string(&config_path)?;
    let context: crate::arch::engine::InstallContext = toml::from_str(&content)?;
    let configuration_sha256 =
        crate::arch::installation_identity::configuration_fingerprint(&content);

    // Exec bypasses the wizard, so nothing has validated the answers a saved
    // (possibly hand-edited) config contains. Fail loudly rather than acting
    // on an invalid or irrelevant answer. Hand-authored configurations may
    // omit wizard provenance; provenance that is present must still be current.
    crate::arch::engine::validate_imported_context(steps, &context)
        .context("Refusing to execute an invalid configuration")?;
    let plan = crate::arch::engine::InstallPlan::try_from(&context)
        .context("Refusing to execute an incomplete or inconsistent installation plan")?;
    let intent_sha256 = crate::arch::installation_identity::fingerprint(&context)?;

    println!("Loaded configuration for user: {}", plan.username.as_str());

    let is_disk_execution = step
        .as_deref()
        .is_none_or(|name| name.eq_ignore_ascii_case("disk"));
    if is_disk_execution
        && !dry_run
        && !is_chroot()
        && let Some(existing) =
            crate::arch::installation_identity::find_matching_installation(&plan, &intent_sha256)?
    {
        println!(
            "This installation configuration is already installed on {} (completed {}).",
            existing.root_device,
            existing.installed_at.format("%Y-%m-%d %H:%M UTC")
        );
        println!("No changes were made; you do not need to install it again.");
        return Ok(ExecutionOutcome::AlreadyInstalled);
    }

    if !dry_run {
        let mut state = InstallState::load_for_configuration(&configuration_sha256);
        state.mark_start();
        state.save()?;
    }

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
        execute_step(
            step_enum,
            &plan,
            &context,
            &executor,
            &config_path,
            &configuration_sha256,
        )
        .await?;
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
            if let Err(e) = execute_step(
                step,
                &plan,
                &context,
                &executor,
                &config_path,
                &configuration_sha256,
            )
            .await
            {
                // A failed offline install still leaves the target
                // referencing the bundle; strip those references even on the
                // way out so a later boot is not broken too.
                if !dry_run
                    && let Err(ce) = crate::arch::offline::cleanup_target(
                        &executor,
                        install_mode,
                        plan.mirror_region.as_deref(),
                    )
                {
                    println!("Warning: Offline cleanup after failed install failed: {ce}");
                }
                return Err(e);
            }
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

            let chroot_state = paths::chroot_path(paths::STATE_FILE);
            if chroot_state.exists()
                && let Err(error) = std::fs::remove_file(&chroot_state)
            {
                println!(
                    "Warning: Failed to remove execution state from target system: {}",
                    error
                );
            }

            let chroot_bin = paths::chroot_path("/usr/bin/ins-install");
            if chroot_bin.exists()
                && let Err(e) = std::fs::remove_file(&chroot_bin)
            {
                println!(
                    "Warning: Failed to remove installer binary from chroot: {}",
                    e
                );
            }

            let marker = crate::arch::installation_identity::write_completed_marker(
                std::path::Path::new(paths::CHROOT_MOUNT),
                &intent_sha256,
            )?;
            println!("Recorded completed installation in {}.", marker.display());

            // Offline installs: drop the bundle references from the target's
            // pacman files and release the bind before declaring completion.
            if let Err(e) = crate::arch::offline::cleanup_target(
                &executor,
                install_mode,
                plan.mirror_region.as_deref(),
            ) {
                println!("Warning: Offline cleanup failed: {e}");
            }
        }
    }

    // Only a full installation reports a total; a single step invoked via
    // `arch exec <step>` (including every chroot re-invocation) reports its
    // own step duration instead.
    if !dry_run && full_installation {
        let elapsed = installation_started.elapsed();
        println!(
            "Installation completed in {}m {:02}s",
            elapsed.as_secs() / 60,
            elapsed.as_secs() % 60
        );
        executor.log(&format!(
            "Installation completed in {:.1}s",
            elapsed.as_secs_f64()
        ));
    }

    Ok(ExecutionOutcome::Completed)
}

async fn execute_step(
    step: InstallStep,
    plan: &crate::arch::engine::InstallPlan,
    context: &crate::arch::engine::InstallContext,
    executor: &dyn CommandRunner,
    config_path: &std::path::Path,
    configuration_sha256: &str,
) -> Result<()> {
    let in_chroot = is_chroot();
    let requires_chroot = step.requires_chroot();
    let step_started = Instant::now();

    // Load state
    let mut state = InstallState::load_for_configuration(configuration_sha256);

    // Check if already complete
    if state.is_complete(step) && !executor.dry_run() {
        println!("Step {:?} is already complete. Skipping.", step);
        return Ok(());
    }

    // Check dependencies
    if let Err(missing) = state.check_dependencies(step) {
        if executor.dry_run() {
            println!(
                "Warning: Missing dependencies for {:?}: {:?}. Proceeding (Dry Run).",
                step, missing
            );
        } else {
            anyhow::bail!("Missing dependencies for {:?}: {:?}", step, missing);
        }
    }

    if requires_chroot && !in_chroot && !executor.dry_run() {
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
            .arg("/usr/bin/ins-install")
            .arg("arch")
            .arg("exec")
            .arg(step_name)
            .arg("--questions-file")
            .arg(paths::CONFIG_FILE);

        if executor.dry_run() {
            // Pass dry-run flag if we are dry-running
            cmd.arg("--dry-run");
        }

        executor.run(&mut cmd)?;

        // Collect logs from chroot
        if !executor.dry_run() {
            let chroot_log = paths::chroot_path(paths::LOG_FILE);
            if chroot_log.exists()
                && let Ok(content) = std::fs::read_to_string(&chroot_log)
            {
                executor.log(&format!("--- BEGIN CHROOT LOG ({:?}) ---", step));
                executor.log(&content);
                executor.log(&format!("--- END CHROOT LOG ({:?}) ---", step));

                // Remove the chroot log file to avoid duplication in subsequent steps
                let _ = std::fs::remove_file(&chroot_log);
            }
        }

        // The step's real runtime was reported by the inner invocation inside
        // the chroot; here the whole arch-chroot round trip only goes to the
        // log file to keep the terminal free of double reports.
        executor.log(&format!(
            "Step {:?} (via chroot) took {:.1}s",
            step,
            step_started.elapsed().as_secs_f64()
        ));

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
        InstallStep::Disk => disk::prepare_disk(plan, context, executor)?,
        InstallStep::Base => base::install_base(plan, executor).await?,
        InstallStep::Fstab => fstab::generate_fstab(executor)?,
        InstallStep::Config => {
            // setup_chroot is handled above if needed
            config::install_config(plan, executor).await?
        }
        InstallStep::Bootloader => {
            // setup_chroot is handled above if needed
            bootloader::install_bootloader(plan, executor).await?
        }
        InstallStep::Post => {
            // setup_chroot is handled above if needed
            post::install_post(plan, executor).await?
        }
    }

    if !executor.dry_run() {
        let elapsed = step_started.elapsed();
        println!("Step {:?} completed in {:.0}s", step, elapsed.as_secs_f64());
        executor.log(&format!(
            "Step {:?} took {:.1}s",
            step,
            elapsed.as_secs_f64()
        ));

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

fn setup_chroot(executor: &dyn CommandRunner, config_path: &std::path::Path) -> Result<()> {
    println!("Setting up chroot environment...");

    // Copy binary
    let current_exe = std::env::current_exe()?;
    let target_bin = paths::chroot_path("/usr/bin/ins-install");

    if executor.dry_run() {
        println!("[DRY RUN] cp {:?} {:?}", current_exe, target_bin);
    } else {
        // Ensure chroot root and /usr/bin exist to avoid 'No such file or directory'
        let chroot_root = std::path::Path::new(paths::CHROOT_MOUNT);
        if !chroot_root.exists() {
            anyhow::bail!(
                "Chroot mount point {} is missing. Ensure the target filesystem is mounted (Disk/Base/Fstab steps).",
                paths::CHROOT_MOUNT
            );
        }

        if let Some(parent) = target_bin.parent()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent)
                .context("Failed to create /usr/bin in chroot before copying binary")?;
        }

        if !current_exe.exists() {
            anyhow::bail!("Installer binary not found at {:?}", current_exe);
        }

        std::fs::copy(&current_exe, &target_bin).with_context(|| {
            format!(
                "Failed to copy binary to chroot (from {:?} to {:?})",
                current_exe, target_bin
            )
        })?;
    }

    // Copy config
    let target_config = paths::chroot_path(paths::CONFIG_FILE);
    if executor.dry_run() {
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
        if executor.dry_run() {
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

    // Offline installs: give the chroot the same view of the bundle as the
    // live system (all file:// paths resolve through this bind) and bring
    // the dotfiles snapshot along for the clone.
    crate::arch::offline::bind_bundle(executor, crate::arch::offline::mode())?;
    crate::arch::offline::copy_dotfiles_snapshot(executor, crate::arch::offline::mode())?;

    Ok(())
}

#[cfg(test)]
pub mod mock {
    use std::cell::RefCell;
    use std::process::{Command, Output};

    use super::CommandRunner;

    /// A mock command runner that records commands without executing them.
    /// Used for testing execution logic without side effects.
    pub struct MockRunner {
        pub commands: RefCell<Vec<String>>,
    }

    impl MockRunner {
        pub fn new() -> Self {
            Self {
                commands: RefCell::new(Vec::new()),
            }
        }

        pub fn command_log(&self) -> Vec<String> {
            self.commands.borrow().clone()
        }
    }

    impl CommandRunner for MockRunner {
        fn dry_run(&self) -> bool {
            false
        }

        fn run(&self, command: &mut Command) -> anyhow::Result<()> {
            self.commands.borrow_mut().push(format_command(command));
            Ok(())
        }

        fn run_with_input(&self, command: &mut Command, input: &str) -> anyhow::Result<()> {
            self.commands
                .borrow_mut()
                .push(format!("{} <<< '{}'", format_command(command), input));
            Ok(())
        }

        fn run_with_output(&self, command: &mut Command) -> anyhow::Result<Option<Output>> {
            self.commands.borrow_mut().push(format_command(command));
            Ok(None)
        }

        fn log(&self, _message: &str) {
            // no-op in tests
        }
    }

    fn format_command(command: &Command) -> String {
        let program = command.get_program().to_string_lossy();
        let args: Vec<_> = command.get_args().map(|a| a.to_string_lossy()).collect();
        format!("{} {}", program, args.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Output};

    use super::CommandRunner;
    use super::mock::MockRunner;

    /// A runner whose commands always fail, to exercise best-effort handling.
    struct AlwaysFails;

    impl CommandRunner for AlwaysFails {
        fn dry_run(&self) -> bool {
            false
        }

        fn run(&self, _command: &mut Command) -> anyhow::Result<()> {
            anyhow::bail!("simulated failure")
        }

        fn run_with_input(&self, _command: &mut Command, _input: &str) -> anyhow::Result<()> {
            anyhow::bail!("simulated failure")
        }

        fn run_with_output(&self, _command: &mut Command) -> anyhow::Result<Option<Output>> {
            anyhow::bail!("simulated failure")
        }

        fn log(&self, _message: &str) {}
    }

    #[test]
    fn best_effort_swallows_failures_and_reports_them() {
        let mut cmd = Command::new("false");

        assert!(!AlwaysFails.run_best_effort(&mut cmd, "test nicety"));
    }

    #[test]
    fn best_effort_still_runs_the_command() {
        let runner = MockRunner::new();
        let mut cmd = Command::new("true");
        cmd.arg("--version");

        assert!(runner.run_best_effort(&mut cmd, "test nicety"));
        assert_eq!(runner.command_log(), vec!["true --version".to_string()]);
    }
}
