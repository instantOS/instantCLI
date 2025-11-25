use anyhow::{Context, Result};
use std::path::PathBuf;

pub mod base;
pub mod bootloader;
pub mod config;
pub mod disk;
pub mod fstab;
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
}

impl CommandExecutor {
    pub fn new(dry_run: bool) -> Self {
        Self { dry_run }
    }

    pub fn run(&self, command: &mut std::process::Command) -> anyhow::Result<()> {
        if self.dry_run {
            self.print_dry_run(command, None);
            Ok(())
        } else {
            let status = command.status()?;
            if !status.success() {
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
        if self.dry_run {
            self.print_dry_run(command, Some(input));
            Ok(())
        } else {
            use std::io::Write;
            command.stdin(std::process::Stdio::piped());
            command.stdout(std::process::Stdio::piped()); // Capture output to avoid clutter

            let mut child = command.spawn()?;

            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(input.as_bytes())?;
            }

            let status = child.wait()?;
            if !status.success() {
                anyhow::bail!("Command failed: {:?}", command);
            }
            Ok(())
        }
    }

    pub fn run_with_output(
        &self,
        command: &mut std::process::Command,
    ) -> anyhow::Result<Option<std::process::Output>> {
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
) -> Result<()> {
    // Check for force dry-run file
    if std::path::Path::new("/etc/instant/installdryrun").exists() {
        if !dry_run {
            println!("Notice: /etc/instant/installdryrun exists, forcing dry-run mode.");
        }
        dry_run = true;
    }

    if dry_run {
        println!("*** DRY RUN MODE ENABLED - No changes will be made ***");
    }

    let executor = CommandExecutor::new(dry_run);

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
        // arch-chroot /mnt /usr/bin/ins arch exec <step> --config /tmp/install_config.toml
        // Note: we need to pass the step name as string.
        // We can convert enum to string via Debug or Display if implemented, or just match.
        // clap::ValueEnum implements Display/FromStr usually but let's be safe.
        let step_name = format!("{:?}", step).to_lowercase();

        let mut cmd = std::process::Command::new("arch-chroot");
        cmd.arg("/mnt")
            .arg("/usr/bin/ins")
            .arg("arch")
            .arg("exec")
            .arg(step_name)
            .arg("--config")
            .arg("/tmp/install_config.toml");

        if executor.dry_run {
            // Pass dry-run flag if we are dry-running
            cmd.arg("--dry-run");
        }

        executor.run(&mut cmd)?;
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
        _ => {
            println!("Step {:?} not implemented yet", step);
        }
    }

    // Mark complete if successful and not dry run
    if !executor.dry_run {
        state.mark_complete(step);
        if let Err(e) = state.save() {
            println!("Warning: Failed to save install state: {}", e);
        }
    }

    Ok(())
}

fn setup_chroot(executor: &CommandExecutor, config_path: &std::path::Path) -> Result<()> {
    println!("Setting up chroot environment...");

    // Copy binary
    let current_exe = std::env::current_exe()?;
    let target_bin = "/mnt/usr/bin/ins";

    if executor.dry_run {
        println!("[DRY RUN] cp {:?} {}", current_exe, target_bin);
    } else {
        // We assume /mnt/usr/bin exists (created by base install)
        std::fs::copy(&current_exe, target_bin).context("Failed to copy binary to chroot")?;
    }

    // Copy config
    let target_config = "/mnt/tmp/install_config.toml";
    if executor.dry_run {
        println!("[DRY RUN] cp {:?} {}", config_path, target_config);
    } else {
        // We assume /mnt/tmp exists because base install creates the directory structure
        std::fs::copy(config_path, target_config).context("Failed to copy config to chroot")?;
    }

    // Copy state file
    let state_file = "/tmp/instant_install_state.toml";
    let target_state = "/mnt/tmp/instant_install_state.toml";
    if std::path::Path::new(state_file).exists() {
        if executor.dry_run {
            println!("[DRY RUN] cp {} {}", state_file, target_state);
        } else {
            std::fs::copy(state_file, target_state).context("Failed to copy state to chroot")?;
        }
    }

    Ok(())
}
