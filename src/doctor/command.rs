use super::privileges::{PrivilegeError, check_privilege_requirements, escalate_for_fix};
use super::registry::REGISTRY;
use super::{CheckResult, DoctorCheck, DoctorCommands, run_all_checks};
use anyhow::{Result, anyhow};
use colored::*;
use dialoguer::Confirm;

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
    super::print_check_list_table(&checks);
    Ok(())
}

fn print_results(results: &[CheckResult]) {
    super::print_results_table(results);
}

fn show_available_fixes(results: &[CheckResult]) {
    let fixable_failures: Vec<_> = results
        .iter()
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

    let non_fixable_failures: Vec<_> = results
        .iter()
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
    let check = REGISTRY
        .create_check(check_id)
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
    super::print_single_check_result_table(result);
}

async fn fix_single_check(check_id: &str) -> Result<()> {
    let check = REGISTRY
        .create_check(check_id)
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
            println!(
                "Fix for '{}' requires administrator privileges.",
                check.name()
            );

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
    let check_name = check.name();

    // Get the before status
    let before_result = check.execute().await;
    let before_status = before_result.status_text().to_string();

    println!("Applying fix for {}...", check_name.green());

    match check.fix().await {
        Ok(()) => {
            // Get the after status
            let after_result = check.execute().await;
            let after_status = after_result.status_text().to_string();

            super::print_fix_summary_table(check_name, &before_status, &after_status);
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
