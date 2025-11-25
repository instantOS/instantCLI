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

pub async fn execute_installation(config_path: PathBuf, step: Option<String>) -> Result<()> {
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
        execute_step(step_enum, &context).await?;
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
            execute_step(step, &context).await?;
        }
    }

    Ok(())
}

async fn execute_step(
    step: InstallStep,
    _context: &crate::arch::engine::InstallContext,
) -> Result<()> {
    println!("  -> Running step: {:?}", step);
    // TODO: Implement actual logic for each step
    // match step {
    //     InstallStep::Disk => ...
    // }
    Ok(())
}
