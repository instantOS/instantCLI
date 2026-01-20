use super::privileges::check_privilege_requirements;
use super::registry::REGISTRY;
use super::ui::show_available_fixes;
use super::{CheckResult, DoctorCheck, run_all_checks};
use anyhow::{Result, anyhow};

/// Run all health checks and display results
pub async fn run_all_checks_cmd(max_concurrency: usize) -> Result<()> {
    let checks = REGISTRY.all_checks();
    let results = run_all_checks(checks, max_concurrency).await;
    super::print_results_table(&results);

    show_available_fixes(&results);
    Ok(())
}

/// List all available health checks
pub async fn list_available_checks() -> Result<()> {
    let checks = REGISTRY.all_checks();
    super::print_check_list_table(&checks);
    Ok(())
}

/// Execute a single health check with validation and display
pub async fn run_single_check(check_id: &str) -> Result<()> {
    let check = REGISTRY
        .create_check(check_id)
        .ok_or_else(|| anyhow!("Unknown check: {}", check_id))?;

    if let Err(e) = check_privilege_requirements(check.as_ref(), false) {
        return Err(anyhow!("Privilege error: {}", e));
    }

    let result = execute_check_logic(check).await;
    super::print_single_check_result_table(&result);
    Ok(())
}

/// Core check execution logic - executes the check and builds the result
pub async fn execute_check_logic(check: Box<dyn DoctorCheck + Send + Sync>) -> CheckResult {
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
