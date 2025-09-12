use anyhow::{anyhow, Result};
use colored::*;
use dialoguer::Confirm;
use super::{CheckResult, DoctorCheck, DoctorCommands, run_all_checks};
use super::registry::REGISTRY;
use super::privileges::{check_privilege_requirements, escalate_for_fix, PrivilegeError};

pub async fn handle_doctor_command(command: Option<DoctorCommands>) -> Result<()> {
    match command {
        None => run_all_checks_cmd().await,
        Some(DoctorCommands::List) => list_available_checks().await,
        Some(DoctorCommands::Run { name }) => run_single_check(&name).await,
        Some(DoctorCommands::Fix { name }) => fix_single_check(&name).await,
    }
}

async fn run_all_checks_cmd() -> Result<()> {
    let checks = REGISTRY.all_checks();
    let results = run_all_checks(checks).await;
    print_results(&results);
    
    // Show available fixes (only for fixable failures)
    show_available_fixes(&results);
    Ok(())
}

async fn list_available_checks() -> Result<()> {
    let checks = REGISTRY.all_checks();
    
    println!("{}", "Available Health Checks:".bold());
    println!();
    
    let header = format!(
        "{: <20} {: <35} {}",
        "ID".bold(),
        "Name".bold(),
        "Description".bold()
    );
    println!("{header}");
    println!("{}", "-".repeat(80));
    
    for check in checks {
        let privileges = match (check.check_privilege_level(), check.fix_privilege_level()) {
            (super::PrivilegeLevel::Any, super::PrivilegeLevel::Any) => "",
            (super::PrivilegeLevel::Any, super::PrivilegeLevel::User) => " (fix: user)",
            (super::PrivilegeLevel::Any, super::PrivilegeLevel::Root) => " (fix: root)",
            (super::PrivilegeLevel::User, super::PrivilegeLevel::User) => " (user only)",
            (super::PrivilegeLevel::Root, _) => " (root required)",
            _ => " (mixed privileges)",
        };
        
        let fix_available = if check.fix_message().is_some() { "✓" } else { "✗" };
        
        let line = format!(
            "{: <20} {: <35} {}{}",
            check.id().cyan(),
            check.name(),
            format!("Fix: {}", fix_available),
            privileges.dimmed()
        );
        println!("{line}");
    }
    
    println!();
    println!("Usage:");
    println!("  instant doctor run <id>    Run a specific check");
    println!("  instant doctor fix <id>    Apply fix for a specific check");
    println!("  instant doctor             Run all checks");
    
    Ok(())
}

fn print_results(results: &[CheckResult]) {
    println!("{}", "System Health Check Results:".bold());
    println!();
    
    let header = format!(
        "{: <35} {: <8} {}",
        "Check".bold(),
        "Status".bold(),
        "Message".bold()
    );
    println!("{header}");
    println!("{}", "-".repeat(55));
    for result in results {
        let status_str = result.status.color_status();
        let fixable_str = result.status.fixable_indicator();
        
        // Color-code the check name based on status
        let check_name = match result.status {
            super::CheckStatus::Pass(_) => result.name.green(),
            super::CheckStatus::Fail { .. } => result.name.red(),
            super::CheckStatus::Warning { .. } => result.name.yellow(),
        };
        
        let line = format!(
            "{: <35} {: <8} {}",
            check_name,
            status_str,
            format!("{}{}", result.status.message(), fixable_str)
        );
        println!("{line}");
    }
    
    println!();
}

fn show_available_fixes(results: &[CheckResult]) {
    let fixable_failures: Vec<_> = results.iter()
        .filter(|result| result.status.needs_fix() && result.status.is_fixable())
        .collect();
    
    if !fixable_failures.is_empty() {
        let fixes_msg = "\nAvailable fixes:".bold().yellow();
        println!("{fixes_msg}");
        for result in &fixable_failures {
            if let Some(ref msg) = result.fix_message {
                println!("  - {}: {}", result.name, msg);
                println!("    Run: instant doctor fix {}", result.check_id);
            }
        }
    }
    
    let non_fixable_failures: Vec<_> = results.iter()
        .filter(|result| result.status.needs_fix() && !result.status.is_fixable())
        .collect();
        
    if !non_fixable_failures.is_empty() {
        let manual_msg = "\nRequires manual intervention:".bold().red();
        println!("{manual_msg}");
        for result in &non_fixable_failures {
            println!("  - {}: {}", result.name, result.status.message());
        }
    }
}

async fn run_single_check(check_id: &str) -> Result<()> {
    let check = REGISTRY.create_check(check_id)
        .ok_or_else(|| anyhow!("Unknown check: {}", check_id))?;
    
    // Verify privilege requirements for check
    if let Err(e) = check_privilege_requirements(check.as_ref(), false) {
        return Err(anyhow!("Privilege error: {}", e));
    }
    
    let result = execute_single_check(check).await;
    print_single_result(&result);
    Ok(())
}

async fn execute_single_check(check: Box<dyn DoctorCheck + Send + Sync>) -> CheckResult {
    let name = check.name().to_string();
    let check_id = check.id().to_string();
    let status = check.execute().await;
    let fix_message = check.fix_message();
    
    CheckResult {
        name,
        check_id,
        status,
        fix_message,
    }
}

fn print_single_result(result: &CheckResult) {
    let status_str = result.status.color_status();
    let fixable_str = result.status.fixable_indicator();
    
    println!(
        "{}: [{}] {}{}",
        result.name.bold(),
        status_str,
        result.status.message(),
        fixable_str
    );
    
    if result.status.needs_fix() {
        if result.status.is_fixable() {
            if let Some(ref msg) = result.fix_message {
                println!("  Fix available: {}", msg);
                println!("  Run: instant doctor fix {}", result.check_id);
            }
        } else {
            println!("  Manual intervention required.");
        }
    }
}

async fn fix_single_check(check_id: &str) -> Result<()> {
    let check = REGISTRY.create_check(check_id)
        .ok_or_else(|| anyhow!("Unknown check: {}", check_id))?;
    
    // STEP 1: Always run the check first to determine current state
    println!("Checking current state for '{}'...", check.name());
    let check_result = check.execute().await;
    
    // STEP 2: Determine if fix is needed based on check result
    if check_result.is_success() {
        println!("✓ {}: {}", check.name(), check_result.message());
        println!("No fix needed - check already passes.");
        return Ok(());
    }
    
    if !check_result.is_fixable() {
        println!("✗ {}: {}", check.name(), check_result.message());
        return Err(anyhow!(
            "Check '{}' failed but is not fixable. Manual intervention required.", 
            check.name()
        ));
    }
    
    // STEP 3: Check is failing and fixable, proceed with fix
    println!("⚠ {}: {}", check.name(), check_result.message());
    println!("Fix is available and will be applied.");
    
    // Check if we have the right privileges for the fix
    match check_privilege_requirements(check.as_ref(), true) {
        Ok(()) => {
            // We have correct privileges, run the fix
            apply_fix(check).await
        }
        Err(PrivilegeError::NeedRoot) => {
            // Need to escalate privileges
            println!("Fix for '{}' requires administrator privileges.", check.name());
            
            if should_escalate(check.as_ref())? {
                escalate_for_fix(check_id)?;
                // This won't return - process will be restarted with sudo
                unreachable!()
            } else {
                println!("Fix cancelled by user.");
                Ok(())
            }
        }
        Err(PrivilegeError::MustNotBeRoot) => {
            Err(anyhow!("Fix for '{}' cannot run as root", check.name()))
        }
    }
}

async fn apply_fix(check: Box<dyn DoctorCheck + Send + Sync>) -> Result<()> {
    println!("Applying fix for {}...", check.name().green());
    
    match check.fix().await {
        Ok(()) => {
            println!("✓ Fix applied successfully for {}", check.name().green());
            Ok(())
        }
        Err(e) => {
            eprintln!(
                "✗ Failed to apply fix for {}: {}",
                check.name(),
                e.to_string().red()
            );
            Err(e)
        }
    }
}

fn should_escalate(check: &dyn DoctorCheck) -> Result<bool> {
    let message = format!(
        "Apply fix for '{}'? This requires administrator privileges.\nFix: {}",
        check.name(),
        check.fix_message().unwrap_or_default()
    );
    
    Ok(Confirm::new()
        .with_prompt(message)
        .default(false)
        .interact()?)
}
