use super::privileges::{PrivilegeError, check_privilege_requirements, escalate_for_fix};
use super::registry::REGISTRY;
use super::ui::{
    DoctorMenuItem, DoctorSummary, FixableIssue, MenuAction, build_fix_menu_items, doctor_header,
    should_escalate, show_all_check_results,
};
use super::{CheckResult, CheckStatus, DoctorCheck, PrivilegeLevel, run_all_checks};
use crate::menu_utils::{FzfWrapper, MenuCursor};
use crate::ui::nerd_font::NerdFont;
use crate::ui::{Level, prelude::*};
use anyhow::{Context, Result, anyhow, bail, ensure};

#[derive(Debug, Clone, PartialEq, Eq)]
struct FixTarget {
    check_id: String,
    name: String,
    priority: u8,
}

impl FixTarget {
    fn from_result(result: &super::CheckResult) -> Self {
        Self {
            check_id: result.check_id.clone(),
            name: result.name.clone(),
            priority: result.status.sort_priority(),
        }
    }

    fn from_issue(issue: &FixableIssue) -> Option<Self> {
        Some(Self {
            check_id: issue.check_id.clone()?,
            name: issue.name.clone(),
            priority: status_priority(&issue.status),
        })
    }
}

#[derive(Debug, Default)]
struct FixPlan {
    user: Vec<FixTarget>,
    any: Vec<FixTarget>,
    root: Vec<FixTarget>,
}

impl FixPlan {
    fn from_results(results: &[super::CheckResult]) -> Result<Self> {
        let mut plan = Self::default();
        for result in results.iter().filter(|result| is_fixable_failure(result)) {
            plan.push(FixTarget::from_result(result))?;
        }
        plan.sort_by_priority();
        Ok(plan)
    }

    fn from_issues(issues: &[FixableIssue]) -> Result<Self> {
        let mut plan = Self::default();
        for target in issues.iter().filter_map(FixTarget::from_issue) {
            plan.push(target)?;
        }
        Ok(plan)
    }

    fn push(&mut self, target: FixTarget) -> Result<()> {
        let check = REGISTRY
            .create_check(&target.check_id)
            .with_context(|| format!("Doctor check '{}' is not registered", target.check_id))?;

        match check.fix_privilege_level() {
            PrivilegeLevel::User => self.user.push(target),
            PrivilegeLevel::Any => self.any.push(target),
            PrivilegeLevel::Root => self.root.push(target),
        }
        Ok(())
    }

    fn sort_by_priority(&mut self) {
        self.user.sort_by_key(|target| target.priority);
        self.any.sort_by_key(|target| target.priority);
        self.root.sort_by_key(|target| target.priority);
    }

    fn is_empty(&self) -> bool {
        self.user.is_empty() && self.any.is_empty() && self.root.is_empty()
    }

    fn len(&self) -> usize {
        self.user.len() + self.any.len() + self.root.len()
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct FixSummary {
    succeeded: usize,
    failed: usize,
}

impl std::ops::AddAssign for FixSummary {
    fn add_assign(&mut self, other: Self) {
        self.succeeded += other.succeeded;
        self.failed += other.failed;
    }
}

fn status_priority(status: &str) -> u8 {
    match status {
        "FAIL" => 0,
        "WARN" => 1,
        _ => 2,
    }
}

fn is_fixable_failure(result: &super::CheckResult) -> bool {
    result.status.is_fixable()
        && matches!(
            result.status,
            CheckStatus::Fail { .. } | CheckStatus::Warning { .. }
        )
}

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
            return Ok(());
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

    fix_check(check, true).await
}

async fn fix_check(
    check: Box<dyn DoctorCheck + Send + Sync>,
    explain_when_not_needed: bool,
) -> Result<()> {
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
        if explain_when_not_needed {
            emit(
                Level::Info,
                "doctor.fix.not_needed",
                &format!(
                    "{} No fix needed - check already passes.",
                    char::from(NerdFont::Info)
                ),
                None,
            );
        }
        return Ok(());
    }

    if check_result.is_skipped() {
        bail!(
            "Check '{}' was skipped: {}",
            check.name(),
            check_result.message()
        );
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
        bail!(
            "Check '{}' failed but is not fixable. Manual intervention required.",
            check.name()
        );
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
            let after_result = check.execute().await;
            let after_status = after_result.status_text().to_string();

            super::print_fix_summary_table(check_name, before_status, &after_status);

            if after_result.is_success() {
                Ok(())
            } else {
                bail!(
                    "Fix for '{}' completed, but the check still reports {}: {}",
                    check_name,
                    after_status,
                    after_result.message()
                )
            }
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

async fn fix_registered_check(check_id: &str) -> Result<()> {
    let check = REGISTRY
        .create_check(check_id)
        .ok_or_else(|| anyhow!("Unknown check: {}", check_id))?;
    fix_check(check, false).await
}

fn parse_batch_ids(batch_ids: &str) -> Result<Vec<String>> {
    let mut seen = std::collections::HashSet::new();
    let ids: Vec<String> = batch_ids
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(|id| {
            ensure!(seen.insert(id), "Duplicate doctor check ID '{id}' in batch");
            Ok(id.to_owned())
        })
        .collect::<Result<_>>()?;
    ensure!(!ids.is_empty(), "Doctor fix batch contains no check IDs");
    Ok(ids)
}

async fn run_fix_group(targets: &[FixTarget]) -> FixSummary {
    let mut summary = FixSummary::default();

    for target in targets {
        emit(
            Level::Info,
            "doctor.fix.plan.item",
            &format!("\nFixing: {} ({})", target.name, target.check_id),
            None,
        );

        match fix_registered_check(&target.check_id).await {
            Ok(()) => {
                emit(
                    Level::Success,
                    "doctor.fix.plan.success",
                    &format!("{} Fixed {}", char::from(NerdFont::Check), target.name),
                    None,
                );
                summary.succeeded += 1;
            }
            Err(error) => {
                emit(
                    Level::Error,
                    "doctor.fix.plan.failed",
                    &format!(
                        "{} Failed to fix {}: {}",
                        char::from(NerdFont::CrossCircle),
                        target.name,
                        error
                    ),
                    None,
                );
                summary.failed += 1;
            }
        }
    }

    summary
}

async fn execute_fix_plan(plan: FixPlan, is_root: bool) -> Result<()> {
    let mut summary = FixSummary::default();

    if is_root && !plan.user.is_empty() {
        emit(
            Level::Warn,
            "doctor.fix.plan.skip_user",
            &format!(
                "{} Skipping {} user-only check(s) - must run as regular user",
                char::from(NerdFont::Warning),
                plan.user.len()
            ),
            None,
        );
    } else if !plan.user.is_empty() {
        announce_fix_group("user-only", plan.user.len());
        summary += run_fix_group(&plan.user).await;
    }

    if !plan.any.is_empty() {
        announce_fix_group("any-level", plan.any.len());
        summary += run_fix_group(&plan.any).await;
    }

    if is_root && !plan.root.is_empty() {
        announce_fix_group("root-required", plan.root.len());
        summary += run_fix_group(&plan.root).await;
    } else if !plan.root.is_empty() {
        emit(
            Level::Info,
            "doctor.fix.plan.escalate",
            &format!(
                "\n{} {} root-required check(s) need administrator privileges",
                char::from(NerdFont::Warning),
                plan.root.len()
            ),
            None,
        );
        for target in &plan.root {
            emit(
                Level::Info,
                "doctor.fix.plan.root_item",
                &format!("Will fix: {} ({})", target.name, target.check_id),
                None,
            );
        }
        escalate_for_fix(
            plan.root
                .into_iter()
                .map(|target| target.check_id)
                .collect(),
        )?;
        emit(
            Level::Success,
            "doctor.fix.plan.root_complete",
            &format!(
                "{} Privileged fixes completed.",
                char::from(NerdFont::Check)
            ),
            None,
        );
        return Ok(());
    }

    emit_fix_summary(summary);
    Ok(())
}

fn announce_fix_group(label: &str, count: usize) {
    emit(
        Level::Info,
        "doctor.fix.plan.group",
        &format!(
            "\n{} Fixing {} {} check(s)...",
            char::from(NerdFont::Info),
            count,
            label
        ),
        None,
    );
}

fn emit_fix_summary(summary: FixSummary) {
    emit(
        Level::Info,
        "doctor.fix.plan.summary",
        "\n=== Summary ===",
        None,
    );
    emit(
        Level::Success,
        "doctor.fix.plan.summary_success",
        &format!(
            "{} Successfully fixed: {}",
            char::from(NerdFont::Check),
            summary.succeeded
        ),
        None,
    );

    if summary.failed > 0 {
        emit(
            Level::Error,
            "doctor.fix.plan.summary_failure",
            &format!(
                "{} Failed to fix: {}",
                char::from(NerdFont::CrossCircle),
                summary.failed
            ),
            None,
        );
    }
}

/// Fix a batch of checks (internal mode used after privilege escalation)
pub async fn fix_batch_checks(batch_ids: String) -> Result<()> {
    use sudo::RunningAs;

    ensure!(
        matches!(sudo::check(), RunningAs::Root),
        "The internal doctor fix batch must run as root"
    );

    let check_ids = parse_batch_ids(&batch_ids)?;
    let mut targets = Vec::with_capacity(check_ids.len());
    for check_id in check_ids {
        let check = REGISTRY
            .create_check(&check_id)
            .with_context(|| format!("Doctor check '{check_id}' is not registered"))?;
        ensure!(
            check.fix_privilege_level() == PrivilegeLevel::Root,
            "Doctor check '{check_id}' does not require root privileges"
        );
        targets.push(FixTarget {
            name: check.name().to_string(),
            check_id,
            priority: 0,
        });
    }

    emit(
        Level::Info,
        "doctor.fix.plan.root",
        &format!(
            "{} Batch fixing {} check(s) that require elevated privileges...",
            char::from(NerdFont::Info),
            targets.len()
        ),
        None,
    );

    let summary = run_fix_group(&targets).await;
    emit_fix_summary(summary);
    Ok(())
}

/// Apply fixes for all failing/fixable health checks
pub async fn fix_all_checks(max_concurrency: usize) -> Result<()> {
    use sudo::RunningAs;

    let checks = REGISTRY.all_checks();
    let results = run_all_checks(checks, max_concurrency).await;

    let plan = FixPlan::from_results(&results)?;

    if plan.is_empty() {
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
            plan.len()
        ),
        None,
    );

    let is_root = matches!(sudo::check(), RunningAs::Root);
    execute_fix_plan(plan, is_root).await
}

/// Interactive fix mode: show menu of fixable issues and apply selected fixes.
///
/// The menu stays alive across fixes: after each run the checks are executed
/// again, the outcome is shown in a dialog, and a refreshed menu is displayed.
pub async fn fix_interactive(max_concurrency: usize) -> Result<()> {
    use super::ui::run_success_menu;

    let mut results = run_all_checks(REGISTRY.all_checks(), max_concurrency).await;
    let mut cursor = MenuCursor::new();

    loop {
        let summary = DoctorSummary::from_results(&results);
        let fixable_issues: Vec<_> = results
            .iter()
            .filter(|r| {
                r.status.is_fixable()
                    && (matches!(r.status, CheckStatus::Fail { .. })
                        || matches!(r.status, CheckStatus::Warning { .. }))
            })
            .map(FixableIssue::from_check_result)
            .collect();
        let manual_issues: Vec<_> = results
            .iter()
            .filter(|r| {
                !r.status.is_fixable()
                    && (matches!(r.status, CheckStatus::Fail { .. })
                        || matches!(r.status, CheckStatus::Warning { .. }))
            })
            .map(FixableIssue::from_check_result)
            .collect();

        if fixable_issues.is_empty() && manual_issues.is_empty() {
            return run_success_menu(&results).await;
        }

        let all_issues = fixable_issues.clone();
        let menu_items = build_fix_menu_items(fixable_issues, manual_issues);
        let subtitle = if summary.fixable > 0 {
            "Select issues to fix, or press Esc to go back"
        } else {
            "No automatic fixes available; review manual issues or press Esc to go back"
        };

        let mut builder = FzfWrapper::builder()
            .prompt("Select issues to fix:")
            .header(doctor_header(
                NerdFont::ShieldCheck,
                &summary,
                "System Diagnostics",
                subtitle,
            ))
            .responsive_layout();

        if let Some(index) = cursor.initial_index(&menu_items) {
            builder = builder.initial_index(index);
        }

        match builder.items(menu_items.clone()).select_many()? {
            crate::menu_utils::DialogOutcome::Submitted(sel) => {
                let selected = sel.items;

                if let Some(item) = selected.first() {
                    cursor.update(item, &menu_items);
                }

                // Handle Back before filtering down to issue items. Otherwise
                // selecting Back produces an empty fix plan and an error.
                if selected
                    .iter()
                    .any(|item| item.is_action(MenuAction::Close))
                {
                    emit(
                        Level::Info,
                        "doctor.fix_choose.back",
                        &format!(
                            "{} Returning to the previous menu",
                            char::from(NerdFont::Info)
                        ),
                        None,
                    );
                    return Ok(());
                }

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
                    let before = results.clone();
                    let fix_result = fix_selected_checks(all_issues).await;
                    results = run_all_checks(REGISTRY.all_checks(), max_concurrency).await;
                    let fix_error = fix_result.err();
                    show_fix_result_dialog(&before, &results, fix_error.as_ref());
                    continue;
                }

                let mut issues = Vec::new();
                let mut selected_manual = Vec::new();
                for item in selected {
                    match item {
                        DoctorMenuItem::Issue(issue) if issue.check_id.is_some() => {
                            issues.push(issue);
                        }
                        DoctorMenuItem::Manual(issue) => selected_manual.push(issue),
                        _ => {}
                    }
                }

                if issues.is_empty() {
                    if selected_manual.is_empty() {
                        // Only action rows were selected. Treat this like Back
                        // instead of failing with an empty fix plan.
                        return Ok(());
                    }
                    show_manual_issues(&selected_manual);
                    continue;
                }

                let before = results.clone();
                let fix_result = fix_selected_checks(issues).await;
                results = run_all_checks(REGISTRY.all_checks(), max_concurrency).await;
                let fix_error = fix_result.err();
                show_fix_result_dialog(&before, &results, fix_error.as_ref());

                if !selected_manual.is_empty() {
                    show_manual_issues(&selected_manual);
                }
            }
            crate::menu_utils::DialogOutcome::Cancelled => {
                emit(
                    Level::Info,
                    "doctor.fix_choose.cancelled",
                    &format!("{} Fix selection cancelled", char::from(NerdFont::Info)),
                    None,
                );
                return Ok(());
            }
        }
    }
}

fn fix_result_summary(
    before: &[CheckResult],
    after: &[CheckResult],
    error: Option<&anyhow::Error>,
) -> String {
    let before_by_id: std::collections::HashMap<&str, &CheckStatus> = before
        .iter()
        .map(|result| (result.check_id.as_str(), &result.status))
        .collect();

    let resolved_checks: Vec<_> = after
        .iter()
        .filter(|result| {
            result.status.is_success()
                && before_by_id
                    .get(result.check_id.as_str())
                    .is_some_and(|status| !status.is_success() && !status.is_skipped())
        })
        .collect();
    let remaining_checks: Vec<_> = after
        .iter()
        .filter(|result| {
            matches!(
                result.status,
                CheckStatus::Fail { .. } | CheckStatus::Warning { .. }
            )
        })
        .collect();
    let remaining_failures = after
        .iter()
        .filter(|result| matches!(result.status, CheckStatus::Fail { .. }))
        .count();
    let remaining_warnings = after
        .iter()
        .filter(|result| matches!(result.status, CheckStatus::Warning { .. }))
        .count();
    let skipped = after
        .iter()
        .filter(|result| result.status.is_skipped())
        .count();

    let mut lines = vec![
        "Fix run finished.".to_string(),
        format!("Resolved: {}", resolved_checks.len()),
    ];
    for result in resolved_checks.iter().take(5) {
        lines.push(format!("  ✓ {}", result.name));
    }
    if resolved_checks.len() > 5 {
        lines.push(format!("  … and {} more", resolved_checks.len() - 5));
    }

    lines.push(format!("Remaining failures: {remaining_failures}"));
    lines.push(format!("Remaining warnings: {remaining_warnings}"));
    if !remaining_checks.is_empty() {
        lines.push("Still needs attention:".to_string());
        for result in remaining_checks.iter().take(5) {
            lines.push(format!(
                "  {} {}: {}",
                result.status.status_text(),
                result.name,
                result.status.message()
            ));
        }
        if remaining_checks.len() > 5 {
            lines.push(format!("  … and {} more", remaining_checks.len() - 5));
        }
    }

    if skipped > 0 {
        lines.push(format!("Skipped: {skipped}"));
    }
    if let Some(error) = error {
        lines.push(format!("Fix error: {error}"));
    }
    lines.push("Returning to refreshed diagnostics...".to_string());

    lines.join("\n")
}

fn show_fix_result_dialog(
    before: &[CheckResult],
    after: &[CheckResult],
    error: Option<&anyhow::Error>,
) {
    let message = fix_result_summary(before, after, error);
    let _ = FzfWrapper::builder()
        .message(message)
        .title("Diagnostic Fix Result")
        .message_dialog();
}

fn show_manual_issues(issues: &[FixableIssue]) {
    let mut lines = vec!["These issues need manual attention:".to_string()];

    for issue in issues {
        lines.push(String::new());
        lines.push(format!(
            "{} {}: {}",
            issue.status, issue.name, issue.message
        ));
        if let Some(fix_message) = &issue.fix_message {
            lines.push(String::new());
            lines.push(fix_message.clone());
        }
    }

    let _ = FzfWrapper::builder()
        .message(lines.join("\n"))
        .title("Manual Attention Required")
        .message_dialog();
}

/// Fix a list of selected checks with proper privilege handling
pub async fn fix_selected_checks(selected: Vec<FixableIssue>) -> Result<()> {
    use sudo::RunningAs;

    let is_root = matches!(sudo::check(), RunningAs::Root);
    let plan = FixPlan::from_issues(&selected)?;
    ensure!(!plan.is_empty(), "No doctor checks were selected");
    execute_fix_plan(plan, is_root).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doctor::{CheckResult, CheckStatus};

    fn result(check_id: &str, status: CheckStatus) -> CheckResult {
        CheckResult {
            name: format!("{check_id} name"),
            check_id: check_id.to_string(),
            status,
            fix_message: Some("fix it".to_string()),
            details: None,
        }
    }

    #[test]
    fn fix_plan_partitions_checks_by_fix_privilege() {
        let results = vec![
            result(
                "fzf-version",
                CheckStatus::Fail {
                    message: "old".to_string(),
                    fixable: true,
                },
            ),
            result(
                "swap",
                CheckStatus::Warning {
                    message: "missing".to_string(),
                    fixable: true,
                },
            ),
            result(
                "locale",
                CheckStatus::Fail {
                    message: "invalid".to_string(),
                    fixable: true,
                },
            ),
        ];

        let plan = FixPlan::from_results(&results).unwrap();

        assert_eq!(plan.user[0].check_id, "fzf-version");
        assert_eq!(plan.any[0].check_id, "swap");
        assert_eq!(plan.root[0].check_id, "locale");
        assert_eq!(plan.len(), 3);
    }

    #[test]
    fn fix_plan_keeps_only_fixable_failures_and_orders_failures_first() {
        let results = vec![
            result(
                "git-config",
                CheckStatus::Warning {
                    message: "missing".to_string(),
                    fixable: true,
                },
            ),
            result("bat-cache", CheckStatus::Pass("ready".to_string())),
            result(
                "nerd-font",
                CheckStatus::Fail {
                    message: "missing".to_string(),
                    fixable: false,
                },
            ),
            result(
                "fzf-version",
                CheckStatus::Fail {
                    message: "old".to_string(),
                    fixable: true,
                },
            ),
        ];

        let plan = FixPlan::from_results(&results).unwrap();

        let ids: Vec<&str> = plan
            .user
            .iter()
            .map(|target| target.check_id.as_str())
            .collect();
        assert_eq!(ids, ["fzf-version", "git-config"]);
        assert_eq!(plan.len(), 2);
    }

    #[test]
    fn fix_plan_rejects_unregistered_check_results() {
        let results = vec![result(
            "missing-check",
            CheckStatus::Fail {
                message: "broken".to_string(),
                fixable: true,
            },
        )];

        let error = FixPlan::from_results(&results).unwrap_err();

        assert!(error.to_string().contains("missing-check"));
    }

    #[test]
    fn batch_ids_are_trimmed_and_empty_segments_are_ignored() {
        assert_eq!(
            parse_batch_ids(" locale, ,pacman-cache,").unwrap(),
            ["locale", "pacman-cache"]
        );
    }

    #[test]
    fn batch_ids_reject_empty_and_duplicate_input() {
        assert!(parse_batch_ids(" , ").is_err());
        assert!(parse_batch_ids("locale,locale").is_err());
    }

    #[test]
    fn fix_summaries_accumulate() {
        let mut summary = FixSummary {
            succeeded: 2,
            failed: 1,
        };
        summary += FixSummary {
            succeeded: 3,
            failed: 4,
        };

        assert_eq!(
            summary,
            FixSummary {
                succeeded: 5,
                failed: 5,
            }
        );
    }

    #[test]
    fn fix_result_summary_counts_resolved_and_remaining_issues() {
        let before = vec![
            result(
                "resolved",
                CheckStatus::Fail {
                    message: "broken".to_string(),
                    fixable: true,
                },
            ),
            result(
                "still-broken",
                CheckStatus::Warning {
                    message: "degraded".to_string(),
                    fixable: true,
                },
            ),
            result("already-fine", CheckStatus::Pass("ok".to_string())),
        ];
        let after = vec![
            result("resolved", CheckStatus::Pass("fixed".to_string())),
            result(
                "still-broken",
                CheckStatus::Fail {
                    message: "still broken".to_string(),
                    fixable: true,
                },
            ),
            result("already-fine", CheckStatus::Pass("ok".to_string())),
        ];

        let summary = fix_result_summary(&before, &after, None);

        assert!(summary.contains("Resolved: 1"));
        assert!(summary.contains("Remaining failures: 1"));
        assert!(summary.contains("Remaining warnings: 0"));
    }

    #[test]
    fn fix_result_summary_includes_fix_errors() {
        let before = vec![result(
            "broken",
            CheckStatus::Fail {
                message: "broken".to_string(),
                fixable: true,
            },
        )];
        let error = anyhow::anyhow!("sudo was cancelled");

        let summary = fix_result_summary(&before, &before, Some(&error));

        assert!(summary.contains("Fix error: sudo was cancelled"));
    }
}
