use anyhow::Result;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, clap::ValueEnum, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstallStep {
    /// Prepare disk (partition, format, mount)
    Disk,
    /// Install base system (pacstrap)
    Base,
    /// Generate fstab
    Fstab,
    /// Configure system (timezone, locale, hostname, users)
    Config,
    /// Install bootloader
    Bootloader,
    /// Post-installation setup
    Post,
}

pub mod disk;

pub struct CommandExecutor {
    dry_run: bool,
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

    fn print_dry_run(&self, command: &std::process::Command, input: Option<&str>) {
        let program = command.get_program().to_string_lossy();
        let args: Vec<_> = command.get_args().map(|a| a.to_string_lossy()).collect();
        let cmd_str = format!("{} {}", program, args.join(" "));

        if let Some(input_str) = input {
            println!(
                "[DRY RUN] echo '{}' | {}",
                input_str.replace('\n', "\\n"),
                cmd_str
            );
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
        execute_step(step_enum, &context, &executor).await?;
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
            execute_step(step, &context, &executor).await?;
        }
    }

    Ok(())
}

async fn execute_step(
    step: InstallStep,
    context: &crate::arch::engine::InstallContext,
    executor: &CommandExecutor,
) -> Result<()> {
    let prefix = if executor.dry_run { "[DRY RUN] " } else { "" };
    println!("{}-> Running step: {:?}", prefix, step);

    match step {
        InstallStep::Disk => disk::prepare_disk(context, executor)?,
        _ => {
            println!("Step {:?} not implemented yet", step);
        }
    }
    Ok(())
}
