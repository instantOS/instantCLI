use super::privileges::{PrivilegeError, check_privilege_requirements, escalate_for_fix};
use super::registry::REGISTRY;
use super::ui::{
    DoctorMenuItem, FixableIssue, MenuAction, build_fix_menu_items, should_escalate,
    show_all_check_results,
};
use super::{DoctorCheck, PrivilegeLevel, run_all_checks};
use crate::menu_utils::{FzfResult, FzfWrapper};
use crate::ui::catppuccin::fzf_mocha_args;
use crate::ui::nerd_font::NerdFont;
use crate::ui::{Level, prelude::*};
use anyhow::{Result, anyhow};

/// Fix a single check by ID
pub async fn fix_single_check(check_id: &str) -> Result<()> {
    let check = REGISTRY
        .create_check(check_id)
        .ok_or_else(|| anyhow!("Unknown check: {}", check_id))?;

    let check_priv_status = check_privilege_requirements(check.as_ref(), false).err();
    let fix_priv_status = check_privilege_requirements(check.as_ref(), true).err();

    if matches!(check_priv_status, Some(PrivilegeError::MustNotBeRoot))
        || matches!(fix_priv_status, Some(PrivilegeError::MustNotBeRoot))
    {
        return Err(anyhow!(
            "Check '{}' must be run as a regular user (not root). Please run without sudo.",
            check.name()
        ));
    }

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
            escalate_for_fix(vec![check_id.to_string()])?;
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

/// Apply a fix for a check
pub async fn apply_fix(
    check: Box<dyn DoctorCheck + Send + Sync>,
    before_status: &str,
) -> Result<()> {
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

/// Fix a check without privilege escalation (for use in batch mode when we already have the right privileges)
pub async fn fix_check_without_escalation(check_id: &str) -> Result<()> {
    let check = REGISTRY
        .create_check(check_id)
        .ok_or_else(|| anyhow!("Unknown check: {}", check_id))?;

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
        return Err(anyhow!(
            "Check '{}' failed but is not fixable. Manual intervention required.",
            check.name()
        ));
    }

    let before_status = check_result.status_text().to_string();
    apply_fix(check, &before_status).await
}

/// Fix a batch of checks (internal mode used after privilege escalation)
pub async fn fix_batch_checks(batch_ids: String) -> Result<()> {
    let check_ids: Vec<String> = batch_ids.split(',').map(|s| s.to_string()).collect();

    emit(
        Level::Info,
        "doctor.fix.batch",
        &format!(
            "{} Batch fixing {} check(s) that require elevated privileges...",
            char::from(NerdFont::Info),
            check_ids.len()
        ),
        None,
    );

    let mut success_count = 0;
    let mut failure_count = 0;

    for check_id in check_ids {
        emit(
            Level::Info,
            "doctor.fix.batch.item",
            &format!("\nFixing: {} (escalated)", check_id),
            None,
        );

        match fix_check_without_escalation(&check_id).await {
            Ok(()) => {
                emit(
                    Level::Success,
                    "doctor.fix.batch.success",
                    &format!("{} Fixed {}", char::from(NerdFont::Check), check_id),
                    None,
                );
                success_count += 1;
            }
            Err(e) => {
                emit(
                    Level::Error,
                    "doctor.fix.batch.failed",
                    &format!(
                        "{} Failed to fix {}: {}",
                        char::from(NerdFont::CrossCircle),
                        check_id,
                        e
                    ),
                    None,
                );
                failure_count += 1;
            }
        }
    }

    emit(
        Level::Info,
        "doctor.fix.batch.summary",
        "\n=== Batch Fix Summary ===",
        None,
    );
    emit(
        Level::Success,
        "doctor.fix.batch.summary_success",
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
            "doctor.fix.batch.summary_failure",
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

/// Apply fixes for all failing/fixable health checks
pub async fn fix_all_checks() -> Result<()> {
    use sudo::RunningAs;

    let checks = REGISTRY.all_checks();
    let results = run_all_checks(checks).await;

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

    let is_root = matches!(sudo::check(), RunningAs::Root);

    let mut user_only_checks = Vec::new();
    let mut root_required_checks = Vec::new();
    let mut any_level_checks = Vec::new();

    for &result in &fixable {
        if let Some(check) = REGISTRY.create_check(&result.check_id) {
            match check.fix_privilege_level() {
                PrivilegeLevel::User => user_only_checks.push(result),
                PrivilegeLevel::Root => root_required_checks.push(result),
                PrivilegeLevel::Any => any_level_checks.push(result),
            }
        }
    }

    user_only_checks.sort_by_key(|r| r.status.sort_priority());
    root_required_checks.sort_by_key(|r| r.status.sort_priority());
    any_level_checks.sort_by_key(|r| r.status.sort_priority());

    let mut total_success = 0;
    let mut total_failure = 0;

    if !user_only_checks.is_empty() {
        if is_root {
            emit(
                Level::Warn,
                "doctor.fix_all.skip_user",
                &format!(
                    "{} Skipping {} user-only check(s) - must run as regular user",
                    char::from(NerdFont::Warning),
                    user_only_checks.len()
                ),
                None,
            );
        } else {
            emit(
                Level::Info,
                "doctor.fix_all.user_only",
                &format!(
                    "\n{} Fixing {} user-only check(s)...",
                    char::from(NerdFont::Info),
                    user_only_checks.len()
                ),
                None,
            );

            for result in user_only_checks {
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
                        total_success += 1;
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
                        total_failure += 1;
                    }
                }
            }
        }
    }

    if !any_level_checks.is_empty() {
        emit(
            Level::Info,
            "doctor.fix_all.any_level",
            &format!(
                "\n{} Fixing {} any-level check(s)...",
                char::from(NerdFont::Info),
                any_level_checks.len()
            ),
            None,
        );

        for result in any_level_checks {
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
                    total_success += 1;
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
                    total_failure += 1;
                }
            }
        }
    }

    if !root_required_checks.is_empty() {
        if is_root {
            emit(
                Level::Info,
                "doctor.fix_all.root_direct",
                &format!(
                    "\n{} Fixing {} root-required check(s)...",
                    char::from(NerdFont::Info),
                    root_required_checks.len()
                ),
                None,
            );

            for result in root_required_checks {
                emit(
                    Level::Info,
                    "doctor.fix_all.item",
                    &format!("\nFixing: {} ({})", result.name, result.check_id),
                    None,
                );

                match fix_check_without_escalation(&result.check_id).await {
                    Ok(()) => {
                        emit(
                            Level::Success,
                            "doctor.fix_all.success",
                            &format!("{} Fixed {}", char::from(NerdFont::Check), result.name),
                            None,
                        );
                        total_success += 1;
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
                        total_failure += 1;
                    }
                }
            }
        } else {
            emit(
                Level::Info,
                "doctor.fix_all.root_escalate",
                &format!(
                    "\n{} {} root-required check(s) need administrator privileges",
                    char::from(NerdFont::Warning),
                    root_required_checks.len()
                ),
                None,
            );

            for result in &root_required_checks {
                emit(
                    Level::Info,
                    "doctor.fix_all.root_item",
                    &format!("Will fix: {} ({})", result.name, result.check_id),
                    None,
                );
            }

            let check_ids: Vec<String> = root_required_checks
                .iter()
                .map(|r| r.check_id.clone())
                .collect();

            escalate_for_fix(check_ids)?;
            unreachable!("Process should restart with sudo")
        }
    }

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
            total_success
        ),
        None,
    );

    if total_failure > 0 {
        emit(
            Level::Error,
            "doctor.fix_all.summary_failure",
            &format!(
                "{} Failed to fix: {}",
                char::from(NerdFont::CrossCircle),
                total_failure
            ),
            None,
        );
    }

    Ok(())
}

/// Interactive fix mode: show menu of fixable issues and apply selected fixes
pub async fn fix_interactive() -> Result<()> {
    use super::ui::run_success_menu;

    let checks = REGISTRY.all_checks();
    let results = run_all_checks(checks).await;

    let fixable_issues: Vec<_> = results
        .iter()
        .filter(|r| r.status.is_fixable() && (r.status.needs_fix() || r.status.is_warning()))
        .map(FixableIssue::from_check_result)
        .collect();

    if fixable_issues.is_empty() {
        return run_success_menu(&results).await;
    }

    loop {
        let menu_items = build_fix_menu_items(fixable_issues.clone());

        match FzfWrapper::builder()
            .multi_select(true)
            .prompt("Select issues to fix:")
            .header("System Diagnostics - Fixable Issues\n\nSelect issues to fix or press Esc to cancel")
            .args(fzf_mocha_args())
            .select(menu_items)?
        {
            FzfResult::MultiSelected(selected) => {
                if selected.is_empty() {
                    emit(
                        Level::Info,
                        "doctor.fix_choose.cancelled",
                        &format!("{} No fixes selected", char::from(NerdFont::Info)),
                        None,
                    );
                    return Ok(());
                }

                if selected.iter().any(|i| i.is_action(MenuAction::ViewAll)) {
                    show_all_check_results(&results)?;
                    continue;
                }

                if selected.iter().any(|i| i.is_action(MenuAction::FixAll)) {
                    return fix_all_checks().await;
                }

                let issues: Vec<FixableIssue> = selected
                    .into_iter()
                    .filter_map(|item| {
                        if let DoctorMenuItem::Issue(issue) = item {
                            if issue.check_id.is_some() {
                                return Some(issue);
                            }
                        }
                        None
                    })
                    .collect();

                return fix_selected_checks(issues).await;
            }
            FzfResult::Cancelled => {
                emit(
                    Level::Info,
                    "doctor.fix_choose.cancelled",
                    &format!(
                        "{} Fix selection cancelled",
                        char::from(NerdFont::Info)
                    ),
                    None,
                );
                return Ok(());
            }
            _ => return Ok(()),
        }
    }
}

/// Fix a list of selected checks with proper privilege handling
pub async fn fix_selected_checks(selected: Vec<FixableIssue>) -> Result<()> {
    use sudo::RunningAs;

    let is_root = matches!(sudo::check(), RunningAs::Root);

    let mut user_only: Vec<String> = Vec::new();
    let mut root_required: Vec<String> = Vec::new();
    let mut any_level: Vec<String> = Vec::new();

    for issue in &selected {
        let check_id = match &issue.check_id {
            Some(id) => id,
            None => continue,
        };
        if let Some(check) = REGISTRY.create_check(check_id) {
            match check.fix_privilege_level() {
                PrivilegeLevel::User => user_only.push(check_id.clone()),
                PrivilegeLevel::Root => root_required.push(check_id.clone()),
                PrivilegeLevel::Any => any_level.push(check_id.clone()),
            }
        }
    }

    let mut total_success = 0;
    let mut total_failure = 0;

    if !user_only.is_empty() {
        if is_root {
            emit(
                Level::Warn,
                "doctor.fix_choose.skip_user",
                &format!(
                    "{} Skipping {} user-only check(s) - must run as regular user",
                    char::from(NerdFont::Warning),
                    user_only.len()
                ),
                None,
            );
        } else {
            for check_id in user_only {
                match fix_single_check(&check_id).await {
                    Ok(()) => total_success += 1,
                    Err(e) => {
                        emit(
                            Level::Error,
                            "doctor.fix_choose.failed",
                            &format!("Failed: {}", e),
                            None,
                        );
                        total_failure += 1;
                    }
                }
            }
        }
    }

    if !any_level.is_empty() {
        for check_id in any_level {
            match fix_single_check(&check_id).await {
                Ok(()) => total_success += 1,
                Err(e) => {
                    emit(
                        Level::Error,
                        "doctor.fix_choose.failed",
                        &format!("Failed: {}", e),
                        None,
                    );
                    total_failure += 1;
                }
            }
        }
    }

    if !root_required.is_empty() {
        if is_root {
            for check_id in root_required {
                match fix_check_without_escalation(&check_id).await {
                    Ok(()) => total_success += 1,
                    Err(e) => {
                        emit(
                            Level::Error,
                            "doctor.fix_choose.failed",
                            &format!("Failed: {}", e),
                            None,
                        );
                        total_failure += 1;
                    }
                }
            }
        } else {
            emit(
                Level::Info,
                "doctor.fix_choose.escalate",
                &format!(
                    "{} {} check(s) require administrator privileges",
                    char::from(NerdFont::Warning),
                    root_required.len()
                ),
                None,
            );
            escalate_for_fix(root_required)?;
            unreachable!("Process should restart with sudo");
        }
    }

    emit(
        Level::Info,
        "doctor.fix_choose.summary",
        "\n=== Summary ===",
        None,
    );
    emit(
        Level::Success,
        "doctor.fix_choose.summary_success",
        &format!(
            "{} Successfully fixed: {}",
            char::from(NerdFont::Check),
            total_success
        ),
        None,
    );

    if total_failure > 0 {
        emit(
            Level::Error,
            "doctor.fix_choose.summary_failure",
            &format!(
                "{} Failed to fix: {}",
                char::from(NerdFont::CrossCircle),
                total_failure
            ),
            None,
        );
    }

    Ok(())
}
