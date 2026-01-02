use super::privileges::{PrivilegeError, check_privilege_requirements, escalate_for_fix};
use super::registry::REGISTRY;
use super::{CheckResult, DoctorCheck, DoctorCommands, run_all_checks};
use crate::menu_utils::{ConfirmResult, FzfWrapper};
use crate::ui::{Level, prelude::*};
use anyhow::{Result, anyhow, bail};
use colored::*;

pub async fn handle_doctor_command(command: Option<DoctorCommands>) -> Result<()> {
    match command {
        None => run_all_checks_cmd().await,
        Some(DoctorCommands::List) => list_available_checks().await,
        Some(DoctorCommands::Run { name }) => run_single_check(&name).await,
        Some(DoctorCommands::Fix { name, all }) => {
            if all {
                fix_all_checks().await
            } else if let Some(check_name) = name {
                fix_single_check(&check_name).await
            } else {
                bail!("Either --all or a check name must be provided")
            }
        }
    }
}

async fn run_all_checks_cmd() -> Result<()> {
    let checks = REGISTRY.all_checks();
    let results = run_all_checks(checks).await;
    super::print_results_table(&results);

    // Show available fixes (only for fixable failures)
    show_available_fixes(&results);
    Ok(())
}

async fn list_available_checks() -> Result<()> {
    let checks = REGISTRY.all_checks();
    super::print_check_list_table(&checks);
    Ok(())
}

fn show_available_fixes(results: &[CheckResult]) {
    // Include both fixable failures AND fixable warnings
    let fixable_issues: Vec<_> = results
        .iter()
        .filter(|result| {
            (result.status.needs_fix() || result.status.is_warning()) && result.status.is_fixable()
        })
        .collect();

    let non_fixable_failures: Vec<_> = results
        .iter()
        .filter(|result| result.status.needs_fix() && !result.status.is_fixable())
        .collect();

    match get_output_format() {
        crate::ui::OutputFormat::Json => {
            if !fixable_issues.is_empty() {
                let fixes_data: Vec<_> = fixable_issues
                    .iter()
                    .map(|result| {
                        serde_json::json!({
                            "name": result.name,
                            "id": result.check_id,
                            "fix_message": result.fix_message,
                        })
                    })
                    .collect();

                emit(
                    Level::Info,
                    "doctor.available_fixes",
                    &format!(
                        "{} Available fixes: {} fixable issues detected",
                        char::from(NerdFont::List),
                        fixes_data.len()
                    ),
                    Some(serde_json::json!({
                        "fixable": fixes_data,
                        "count": fixes_data.len(),
                    })),
                );
            }

            if !non_fixable_failures.is_empty() {
                let manual_data: Vec<_> = non_fixable_failures
                    .iter()
                    .map(|result| {
                        serde_json::json!({
                            "name": result.name,
                            "id": result.check_id,
                            "message": result.status.message(),
                        })
                    })
                    .collect();

                emit(
                    Level::Info,
                    "doctor.manual_intervention",
                    &format!(
                        "{} Manual intervention required: {} issues need attention",
                        char::from(NerdFont::Warning),
                        manual_data.len()
                    ),
                    Some(serde_json::json!({
                        "non_fixable": manual_data,
                        "count": manual_data.len(),
                    })),
                );
            }
        }
        crate::ui::OutputFormat::Text => {
            if !fixable_issues.is_empty() {
                let fixes_msg = "\nAvailable fixes:".bold().yellow();
                println!("{fixes_msg}");
                for result in &fixable_issues {
                    if let Some(ref msg) = result.fix_message {
                        println!("  - {}: {}", result.name, msg);
                        println!(
                            "    Run: {} doctor fix {}",
                            env!("CARGO_BIN_NAME"),
                            result.check_id
                        );
                    }
                }
            }

            if !non_fixable_failures.is_empty() {
                let manual_msg = "\nRequires manual intervention:".bold().red();
                println!("{manual_msg}");
                for result in &non_fixable_failures {
                    println!("  - {}: {}", result.name, result.status.message());
                }
            }
        }
    }
}

/// Execute a single health check with validation and display
/// This function handles validation, privilege checking, execution, and result display
async fn run_single_check(check_id: &str) -> Result<()> {
    let check = REGISTRY
        .create_check(check_id)
        .ok_or_else(|| anyhow!("Unknown check: {}", check_id))?;

    // Verify privilege requirements for check
    if let Err(e) = check_privilege_requirements(check.as_ref(), false) {
        return Err(anyhow!("Privilege error: {}", e));
    }

    let result = execute_check_logic(check).await;
    super::print_single_check_result_table(&result);
    Ok(())
}

/// Core check execution logic - executes the check and builds the result
async fn execute_check_logic(check: Box<dyn DoctorCheck + Send + Sync>) -> CheckResult {
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

async fn fix_single_check(check_id: &str) -> Result<()> {
    let check = REGISTRY
        .create_check(check_id)
        .ok_or_else(|| anyhow!("Unknown check: {}", check_id))?;

    // STEP 1: Determine if we need to adjust privileges
    // Treat privilege requirements as first-class outcomes to avoid unsafe escalation.
    let check_priv_status = check_privilege_requirements(check.as_ref(), false).err();
    let fix_priv_status = check_privilege_requirements(check.as_ref(), true).err();

    // If either phase must not run as root, bail out early to avoid accidental root execution.
    if matches!(check_priv_status, Some(PrivilegeError::MustNotBeRoot))
        || matches!(fix_priv_status, Some(PrivilegeError::MustNotBeRoot))
    {
        return Err(anyhow!(
            "Check '{}' must be run as a regular user (not root). Please run without sudo.",
            check.name()
        ));
    }

    // Fix requires root: offer escalation.
    if matches!(fix_priv_status, Some(PrivilegeError::NeedRoot)) {
        emit(
            Level::Warn,
            "doctor.fix.privileges",
            &format!(
                "{} Check '{}' fix requires administrator privileges.",
                char::from(NerdFont::Warning),
                check.name(),
            ),
            None,
        );

        if should_escalate(check.as_ref())? {
            escalate_for_fix(check_id)?;
            unreachable!("Process should restart with sudo")
        } else {
            emit(
                Level::Info,
                "doctor.fix.cancelled",
                &format!("{} Fix cancelled by user.", char::from(NerdFont::Info)),
                None,
            );
            return Ok(());
        }
    }

    // Only the check requires root: do NOT escalate automatically.
    // Running the fix as root could be unsafe for user-session checks.
    if matches!(check_priv_status, Some(PrivilegeError::NeedRoot)) {
        emit(
            Level::Warn,
            "doctor.fix.privileges",
            &format!(
                "{} Check '{}' requires root privileges to run accurately; proceeding without escalation because the fix can run as a regular user.",
                char::from(NerdFont::Warning),
                check.name()
            ),
            None,
        );
    }

    // STEP 2: Run the check to determine current state
    emit(
        Level::Info,
        "doctor.fix.check",
        &format!(
            "{} Checking current state for '{}'...",
            char::from(NerdFont::Info),
            check.name()
        ),
        None,
    );
    let check_result = check.execute().await;

    // STEP 3: Handle check result
    if check_result.is_success() {
        emit(
            Level::Success,
            "doctor.fix.not_needed",
            &format!(
                "{} {}: {}",
                char::from(NerdFont::Check),
                check.name(),
                check_result.message()
            ),
            None,
        );
        emit(
            Level::Info,
            "doctor.fix.not_needed",
            &format!(
                "{} No fix needed - check already passes.",
                char::from(NerdFont::Info)
            ),
            None,
        );
        return Ok(());
    }

    if check_result.is_skipped() {
        return Err(anyhow!(
            "Check '{}' was skipped: {}",
            check.name(),
            check_result.message()
        ));
    }

    if !check_result.is_fixable() {
        emit(
            Level::Error,
            "doctor.fix.not_fixable",
            &format!(
                "{} {}: {}",
                char::from(NerdFont::CrossCircle),
                check.name(),
                check_result.message()
            ),
            None,
        );
        return Err(anyhow!(
            "Check '{}' failed but is not fixable. Manual intervention required.",
            check.name()
        ));
    }

    // STEP 4: Apply the fix
    emit(
        Level::Warn,
        "doctor.fix.available",
        &format!(
            "{} {}: {}",
            char::from(NerdFont::Warning),
            check.name(),
            check_result.message()
        ),
        None,
    );

    let before_status = check_result.status_text().to_string();
    apply_fix(check, &before_status).await
}

async fn apply_fix(check: Box<dyn DoctorCheck + Send + Sync>, before_status: &str) -> Result<()> {
    let check_name = check.name();

    emit(
        Level::Info,
        "doctor.fix.applying",
        &format!(
            "{} Applying fix for {}...",
            char::from(NerdFont::Info),
            check_name
        ),
        None,
    );

    match check.fix().await {
        Ok(()) => {
            // Get the after status
            let after_result = check.execute().await;
            let after_status = after_result.status_text().to_string();

            super::print_fix_summary_table(check_name, before_status, &after_status);
            Ok(())
        }
        Err(e) => {
            emit(
                Level::Error,
                "doctor.fix.failed",
                &format!(
                    "{} Failed to apply fix for {}: {}",
                    char::from(NerdFont::CrossCircle),
                    check.name(),
                    e
                ),
                None,
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

    match FzfWrapper::confirm(&message)
        .map_err(|e| anyhow::anyhow!("Confirmation failed: {}", e))?
    {
        ConfirmResult::Yes => Ok(true),
        ConfirmResult::No | ConfirmResult::Cancelled => Ok(false),
    }
}

/// Apply fixes for all failing/fixable health checks
async fn fix_all_checks() -> Result<()> {
    // STEP 1: Run all checks to identify failures
    let checks = REGISTRY.all_checks();
    let results = run_all_checks(checks).await;

    // STEP 2: Filter for fixable failures (both Fail and Warning)
    let fixable: Vec<_> = results
        .iter()
        .filter(|r| r.status.is_fixable() && (r.status.needs_fix() || r.status.is_warning()))
        .collect();

    if fixable.is_empty() {
        emit(
            Level::Success,
            "doctor.fix_all.none",
            &format!("{} No fixable issues found!", char::from(NerdFont::Check)),
            None,
        );
        return Ok(());
    }

    emit(
        Level::Info,
        "doctor.fix_all.start",
        &format!(
            "{} Found {} fixable issue(s)",
            char::from(NerdFont::List),
            fixable.len()
        ),
        None,
    );

    // STEP 3: Sort by priority (Fail before Warning)
    let mut fixable_sorted = fixable;
    fixable_sorted.sort_by_key(|r| r.status.sort_priority());

    // STEP 4: Apply fixes sequentially, continuing on failure
    let mut success_count = 0;
    let mut failure_count = 0;

    for result in fixable_sorted {
        emit(
            Level::Info,
            "doctor.fix_all.item",
            &format!("\nFixing: {} ({})", result.name, result.check_id),
            None,
        );

        match fix_single_check(&result.check_id).await {
            Ok(()) => {
                emit(
                    Level::Success,
                    "doctor.fix_all.success",
                    &format!("{} Fixed {}", char::from(NerdFont::Check), result.name),
                    None,
                );
                success_count += 1;
            }
            Err(e) => {
                emit(
                    Level::Error,
                    "doctor.fix_all.failed",
                    &format!(
                        "{} Failed to fix {}: {}",
                        char::from(NerdFont::CrossCircle),
                        result.name,
                        e
                    ),
                    None,
                );
                failure_count += 1;
            }
        }
    }

    // STEP 5: Show final summary
    emit(
        Level::Info,
        "doctor.fix_all.summary",
        "\n=== Summary ===",
        None,
    );
    emit(
        Level::Success,
        "doctor.fix_all.summary_success",
        &format!(
            "{} Successfully fixed: {}",
            char::from(NerdFont::Check),
            success_count
        ),
        None,
    );

    if failure_count > 0 {
        emit(
            Level::Error,
            "doctor.fix_all.summary_failure",
            &format!(
                "{} Failed to fix: {}",
                char::from(NerdFont::CrossCircle),
                failure_count
            ),
            None,
        );
    }

    Ok(())
}
