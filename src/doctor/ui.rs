use super::{CheckResult, CheckStatus};
use crate::menu_utils::{
    ConfirmResult, FzfPreview, FzfSelectable, FzfWrapper, HeaderBuilder, MenuCursor,
};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, format_with_color};
use crate::ui::nerd_font::NerdFont;
use crate::ui::prelude::*;
use anyhow::Result;

/// Menu actions for the doctor interactive menu
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    ViewAll,
    FixAll,
    Close,
}

impl std::fmt::Display for MenuAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MenuAction::ViewAll => write!(f, "__VIEW_ALL__"),
            MenuAction::FixAll => write!(f, "__ALL__"),
            MenuAction::Close => write!(f, "__CLOSE__"),
        }
    }
}

/// Counts used by the doctor menu header.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DoctorSummary {
    pub passed: usize,
    pub failures: usize,
    pub warnings: usize,
    pub fixable: usize,
    pub manual: usize,
    pub skipped: usize,
}

impl DoctorSummary {
    pub fn from_results(results: &[CheckResult]) -> Self {
        let mut summary = Self::default();

        for result in results {
            match &result.status {
                CheckStatus::Pass(_) => summary.passed += 1,
                CheckStatus::Fail { fixable, .. } => {
                    summary.failures += 1;
                    if *fixable {
                        summary.fixable += 1;
                    } else {
                        summary.manual += 1;
                    }
                }
                CheckStatus::Warning { fixable, .. } => {
                    summary.warnings += 1;
                    if *fixable {
                        summary.fixable += 1;
                    } else {
                        summary.manual += 1;
                    }
                }
                CheckStatus::Skipped(_) => summary.skipped += 1,
            }
        }

        summary
    }

    pub fn is_healthy(&self) -> bool {
        self.failures == 0 && self.warnings == 0
    }

    fn status_line(&self) -> String {
        let passed = format_with_color(&format!("{} passed", self.passed), colors::GREEN);
        let failures = if self.failures > 0 {
            format_with_color(&format!("{} failures", self.failures), colors::RED)
        } else {
            format_with_color("0 failures", colors::GREEN)
        };
        let warnings = if self.warnings > 0 {
            format_with_color(&format!("{} warnings", self.warnings), colors::YELLOW)
        } else {
            format_with_color("0 warnings", colors::GREEN)
        };
        let fixable = if self.fixable > 0 {
            format_with_color(&format!("{} fixable", self.fixable), colors::BLUE)
        } else {
            format_with_color("0 fixable", colors::OVERLAY1)
        };
        let manual = if self.manual > 0 {
            format_with_color(&format!("{} manual", self.manual), colors::PEACH)
        } else {
            format_with_color("0 manual", colors::OVERLAY1)
        };
        let skipped = format_with_color(&format!("{} skipped", self.skipped), colors::OVERLAY1);

        format!("{passed} · {failures} · {warnings} · {fixable} · {manual} · {skipped}")
    }
}

/// Build the standard doctor menu header with the status summary.
pub fn doctor_header(
    icon: NerdFont,
    summary: &DoctorSummary,
    title: impl AsRef<str>,
    subtitle: impl AsRef<str>,
) -> crate::menu_utils::Header {
    HeaderBuilder::new(icon, title)
        .field("Status", summary.status_line())
        .subtitle(subtitle)
        .build()
}

/// Wrapper enum for menu items that can be either an action or an issue
#[derive(Clone)]
pub enum DoctorMenuItem {
    Action(MenuAction),
    Issue(FixableIssue),
    Manual(FixableIssue),
}

impl DoctorMenuItem {
    pub fn view_all() -> Self {
        DoctorMenuItem::Action(MenuAction::ViewAll)
    }

    pub fn fix_all(count: usize) -> Self {
        DoctorMenuItem::Issue(FixableIssue {
            name: format!("Fix All Issues ({})", count),
            action: Some(MenuAction::FixAll),
            status: "ALL".to_string(),
            message: "Apply all available fixes".to_string(),
            fix_message: None,
            check_id: None,
        })
    }

    pub fn close() -> Self {
        DoctorMenuItem::Action(MenuAction::Close)
    }

    pub fn back() -> Self {
        DoctorMenuItem::Action(MenuAction::Close)
    }

    pub fn is_action(&self, action: MenuAction) -> bool {
        match self {
            DoctorMenuItem::Action(a) => *a == action,
            DoctorMenuItem::Issue(issue) | DoctorMenuItem::Manual(issue) => {
                issue.action == Some(action)
            }
        }
    }
}

impl FzfSelectable for DoctorMenuItem {
    fn fzf_display_text(&self) -> String {
        match self {
            DoctorMenuItem::Action(action) => match action {
                MenuAction::ViewAll => {
                    format!(
                        "{} View All Check Results",
                        format_icon_colored(NerdFont::List, colors::BLUE)
                    )
                }
                MenuAction::FixAll => {
                    format!(
                        "{} Fix All Issues",
                        format_icon_colored(NerdFont::Wrench, colors::GREEN)
                    )
                }
                MenuAction::Close => {
                    format!("{} Back", format_back_icon())
                }
            },
            DoctorMenuItem::Issue(issue) => issue.fzf_display_text(),
            DoctorMenuItem::Manual(issue) => format!(
                "{} {}",
                issue.fzf_display_text(),
                format_with_color("(manual)", colors::OVERLAY1)
            ),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            DoctorMenuItem::Action(action) => {
                use crate::ui::preview::PreviewBuilder;
                match action {
                    MenuAction::ViewAll => PreviewBuilder::new()
                        .header(NerdFont::List, "View All Check Results")
                        .text("Show status of all checks including passed and skipped")
                        .build(),
                    MenuAction::FixAll => PreviewBuilder::new()
                        .header(NerdFont::Wrench, "Fix All Issues")
                        .text("Apply all available fixes")
                        .build(),
                    MenuAction::Close => PreviewBuilder::new()
                        .header(NerdFont::ArrowLeft, "Back")
                        .text("Return to the previous menu")
                        .build(),
                }
            }
            DoctorMenuItem::Issue(issue) => issue.fzf_preview(),
            DoctorMenuItem::Manual(issue) => issue.manual_preview(),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            DoctorMenuItem::Action(action) => action.to_string(),
            DoctorMenuItem::Issue(issue) | DoctorMenuItem::Manual(issue) => issue.fzf_key(),
        }
    }
}

/// Struct representing a fixable issue for FZF menu display
#[derive(Clone)]
pub struct FixableIssue {
    pub name: String,
    pub check_id: Option<String>,
    pub action: Option<MenuAction>,
    pub status: String,
    pub message: String,
    pub fix_message: Option<String>,
}

impl FixableIssue {
    pub fn from_check_result(result: &CheckResult) -> Self {
        Self {
            name: result.name.clone(),
            check_id: Some(result.check_id.clone()),
            action: None,
            status: result.status.status_text().to_string(),
            message: result.status.message().to_string(),
            fix_message: result.fix_message.clone(),
        }
    }

    fn preview(&self, manual: bool) -> FzfPreview {
        use crate::ui::preview::PreviewBuilder;

        let icon = match self.status.as_str() {
            "FAIL" => NerdFont::CrossCircle,
            "WARN" => NerdFont::Warning,
            "ALL" => NerdFont::CheckCircle,
            "INFO" => NerdFont::Info,
            _ => NerdFont::Info,
        };

        let title = if manual {
            format!("{} {} (manual)", self.status, self.name)
        } else {
            format!("{} {}", self.status, self.name)
        };
        let mut builder = PreviewBuilder::new().header(icon, &title);

        if self.action == Some(MenuAction::ViewAll) {
            builder = builder.text(&self.message);
        } else {
            builder = builder.field("Current Status", &self.message);

            if let Some(fix_msg) = &self.fix_message {
                let label = if manual {
                    "Manual remediation:"
                } else {
                    "Available Fix:"
                };
                builder = builder
                    .blank()
                    .line(colors::MAUVE, None, label)
                    .text(fix_msg);
            } else if self.action != Some(MenuAction::FixAll) {
                builder = builder
                    .blank()
                    .subtext("No automatic fix available")
                    .subtext("Manual intervention may be required");
            }

            if self.action != Some(MenuAction::FixAll)
                && let Some(ref id) = self.check_id
            {
                builder = builder.blank().subtext(&format!("ID: {}", id));
            }
        }

        builder.build()
    }

    pub fn manual_preview(&self) -> FzfPreview {
        self.preview(true)
    }
}

impl FzfSelectable for FixableIssue {
    fn fzf_display_text(&self) -> String {
        if let Some(action) = self.action {
            match action {
                MenuAction::ViewAll => {
                    format!(
                        "{} View All Check Results",
                        format_icon_colored(NerdFont::List, colors::BLUE)
                    )
                }
                MenuAction::FixAll => {
                    format!(
                        "{} {}",
                        format_icon_colored(NerdFont::Wrench, colors::GREEN),
                        self.name
                    )
                }
                MenuAction::Close => {
                    format!("{} Back", format_back_icon())
                }
            }
        } else {
            let (icon, color) = match self.status.as_str() {
                "FAIL" => (NerdFont::CrossCircle, colors::RED),
                "WARN" => (NerdFont::Warning, colors::YELLOW),
                _ => (NerdFont::Info, colors::BLUE),
            };
            format!(
                "{} {} {}",
                format_icon_colored(icon, color),
                self.status,
                self.name
            )
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview(false)
    }

    fn fzf_key(&self) -> String {
        if let Some(action) = self.action {
            action.to_string()
        } else {
            self.check_id.clone().unwrap_or_default()
        }
    }
}

/// Struct representing a check result for viewing in the "View All Results" menu
#[derive(Clone)]
pub struct ViewableCheck {
    pub name: String,
    pub check_id: String,
    pub status: String,
    pub message: String,
    pub details: Option<String>,
    pub fix_message: Option<String>,
}

impl ViewableCheck {
    pub fn from_check_result(result: &CheckResult) -> Self {
        Self {
            name: result.name.clone(),
            check_id: result.check_id.clone(),
            status: result.status.status_text().to_string(),
            message: result.status.message().to_string(),
            details: result.details.clone(),
            fix_message: result.fix_message.clone(),
        }
    }

    fn detail_text(&self) -> String {
        let mut lines = vec![
            format!("{} {}", self.status, self.name),
            String::new(),
            format!("Status: {}", self.message),
        ];

        if let Some(details) = &self.details {
            lines.push(String::new());
            lines.push(details.clone());
        }

        if let Some(fix_message) = &self.fix_message {
            lines.push(String::new());
            lines.push("Remediation:".to_string());
            lines.push(fix_message.clone());
        }

        lines.push(String::new());
        lines.push(format!("ID: {}", self.check_id));

        lines.join("\n")
    }
}

impl FzfSelectable for ViewableCheck {
    fn fzf_display_text(&self) -> String {
        let (icon, icon_color, status_color) = match self.status.as_str() {
            "PASS" => (NerdFont::Check, colors::GREEN, colors::GREEN),
            "FAIL" => (NerdFont::CrossCircle, colors::RED, colors::RED),
            "WARN" => (NerdFont::Warning, colors::YELLOW, colors::YELLOW),
            "SKIP" => (NerdFont::Minus, colors::OVERLAY1, colors::OVERLAY1),
            _ => (NerdFont::Info, colors::BLUE, colors::BLUE),
        };

        format!(
            "{} {} {}",
            format_icon_colored(icon, icon_color),
            format_with_color(&self.status, status_color),
            self.name
        )
    }

    fn fzf_preview(&self) -> FzfPreview {
        use crate::ui::preview::PreviewBuilder;

        let icon = match self.status.as_str() {
            "PASS" => NerdFont::Check,
            "FAIL" => NerdFont::CrossCircle,
            "WARN" => NerdFont::Warning,
            "SKIP" => NerdFont::Minus,
            _ => NerdFont::Info,
        };

        let mut builder = PreviewBuilder::new()
            .header(icon, &format!("{} {}", self.status, self.name))
            .field("Status", &self.message)
            .blank();

        if let Some(details) = &self.details {
            builder = builder.text(details).blank();
        }

        if let Some(fix_message) = &self.fix_message {
            builder = builder
                .line(colors::MAUVE, None, "Remediation:")
                .text(fix_message)
                .blank();
        }

        builder.subtext(&format!("ID: {}", self.check_id)).build()
    }

    fn fzf_key(&self) -> String {
        self.check_id.clone()
    }
}

/// Show available fixes (only for fixable failures)
pub fn show_available_fixes(results: &[CheckResult]) {
    use colored::*;

    let fixable_issues: Vec<_> = results
        .iter()
        .filter(|result| {
            (matches!(result.status, CheckStatus::Fail { .. })
                || matches!(result.status, CheckStatus::Warning { .. }))
                && result.status.is_fixable()
        })
        .collect();

    let non_fixable_failures: Vec<_> = results
        .iter()
        .filter(|result| {
            matches!(result.status, CheckStatus::Fail { .. }) && !result.status.is_fixable()
        })
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
                for (i, result) in fixable_issues.iter().enumerate() {
                    if let Some(ref msg) = result.fix_message {
                        let check_name = result.name.bright_cyan();
                        let mut lines = msg.lines();
                        if let Some(first_line) = lines.next() {
                            println!("  {} {}: {}", i + 1, check_name, first_line);
                            for line in lines {
                                println!("     {}", line);
                            }
                        }
                        let run_cmd =
                            format!("{} doctor fix {}", env!("CARGO_BIN_NAME"), result.check_id)
                                .bright_white();
                        println!("     {} {}", "→".green(), run_cmd);
                    }
                }
            }

            if !non_fixable_failures.is_empty() {
                let manual_msg = "\nRequires manual intervention:".bold().red();
                println!("{manual_msg}");
                for (i, result) in non_fixable_failures.iter().enumerate() {
                    let check_name = result.name.bright_magenta();
                    println!("  {} {}: {}", i + 1, check_name, result.status.message());
                }
            }
        }
    }
}

/// Show all check results (including passed and skipped checks) in a menu
pub fn show_all_check_results(results: &[CheckResult]) -> Result<()> {
    let viewable: Vec<_> = results
        .iter()
        .map(ViewableCheck::from_check_result)
        .collect();
    let summary = DoctorSummary::from_results(results);
    let mut cursor = MenuCursor::new();

    loop {
        let mut builder = FzfWrapper::builder()
            .prompt("View results:")
            .header(doctor_header(
                NerdFont::List,
                &summary,
                "All Check Results",
                "Use arrow keys to navigate; Enter shows details, Esc returns",
            ));

        if let Some(index) = cursor.initial_index(&viewable) {
            builder = builder.initial_index(index);
        }

        match builder.items(viewable.clone()).select_one_with_back()? {
            crate::menu_utils::DialogOutcome::Submitted(item) => {
                cursor.update(&item, &viewable);
                let _ = FzfWrapper::builder()
                    .message(item.detail_text())
                    .title("Check Result")
                    .message_dialog();
            }
            crate::menu_utils::DialogOutcome::Cancelled => return Ok(()),
        }
    }
}

/// Ask user if they want to escalate privileges for a fix
pub fn should_escalate(check: &dyn super::DoctorCheck) -> Result<bool> {
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

/// Build menu items for the interactive doctor menu.
pub fn build_fix_menu_items(
    fixable_issues: Vec<FixableIssue>,
    manual_issues: Vec<FixableIssue>,
) -> Vec<DoctorMenuItem> {
    let count = fixable_issues.len();
    let mut menu_items = vec![DoctorMenuItem::view_all()];
    if count > 0 {
        menu_items.push(DoctorMenuItem::fix_all(count));
    }
    menu_items.extend(fixable_issues.into_iter().map(DoctorMenuItem::Issue));
    menu_items.extend(manual_issues.into_iter().map(DoctorMenuItem::Manual));
    menu_items.push(DoctorMenuItem::back());
    menu_items
}

/// Build menu items for the interactive menu when all checks pass
pub fn build_success_menu_items() -> Vec<DoctorMenuItem> {
    vec![DoctorMenuItem::view_all(), DoctorMenuItem::close()]
}

/// Run the interactive menu for when all checks pass
pub async fn run_success_menu(results: &[CheckResult]) -> Result<()> {
    let summary = DoctorSummary::from_results(results);
    let healthy = summary.is_healthy();
    let (title, icon) = if healthy {
        ("All Systems Operational", NerdFont::CheckCircle)
    } else {
        ("Diagnostics Complete", NerdFont::Info)
    };

    let menu_items = build_success_menu_items();
    let mut cursor = MenuCursor::new();

    loop {
        let subtitle = if healthy {
            "Select an option or press Esc to exit"
        } else {
            "Review the results or press Esc to exit"
        };
        let mut builder = FzfWrapper::builder()
            .prompt("Select:")
            .header(doctor_header(icon, &summary, title, subtitle));

        if let Some(index) = cursor.initial_index(&menu_items) {
            builder = builder.initial_index(index);
        }

        match builder.items(menu_items.clone()).select_one()? {
            crate::menu_utils::DialogOutcome::Submitted(item)
                if item.is_action(MenuAction::ViewAll) =>
            {
                cursor.update(&item, &menu_items);
                show_all_check_results(results)?;
                continue;
            }
            crate::menu_utils::DialogOutcome::Submitted(item) => {
                cursor.update(&item, &menu_items);
            }
            crate::menu_utils::DialogOutcome::Cancelled => {}
        }
        return Ok(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(check_id: &str, status: CheckStatus) -> CheckResult {
        CheckResult {
            name: check_id.to_string(),
            check_id: check_id.to_string(),
            status,
            fix_message: None,
            details: None,
        }
    }

    #[test]
    fn summary_counts_failures_warnings_fixable_manual_and_skipped() {
        let results = vec![
            result("pass", CheckStatus::Pass("ok".to_string())),
            result(
                "fixable-fail",
                CheckStatus::Fail {
                    message: "broken".to_string(),
                    fixable: true,
                },
            ),
            result(
                "manual-warning",
                CheckStatus::Warning {
                    message: "manual".to_string(),
                    fixable: false,
                },
            ),
            result("skip", CheckStatus::Skipped("not applicable".to_string())),
        ];

        let summary = DoctorSummary::from_results(&results);

        assert_eq!(summary.passed, 1);
        assert_eq!(summary.failures, 1);
        assert_eq!(summary.warnings, 1);
        assert_eq!(summary.fixable, 1);
        assert_eq!(summary.manual, 1);
        assert_eq!(summary.skipped, 1);
        assert!(!summary.is_healthy());
    }

    #[test]
    fn summary_is_healthy_with_only_passes_and_skips() {
        let results = vec![
            result("pass", CheckStatus::Pass("ok".to_string())),
            result("skip", CheckStatus::Skipped("not applicable".to_string())),
        ];

        assert!(DoctorSummary::from_results(&results).is_healthy());
    }

    #[test]
    fn issue_menu_includes_manual_issues_without_fix_all_when_none_are_fixable() {
        let manual = FixableIssue::from_check_result(&result(
            "manual",
            CheckStatus::Fail {
                message: "needs manual work".to_string(),
                fixable: false,
            },
        ));

        let items = build_fix_menu_items(Vec::new(), vec![manual]);

        assert!(items.iter().any(|item| item.is_action(MenuAction::ViewAll)));
        assert!(items.iter().any(|item| item.is_action(MenuAction::Close)));
        assert!(!items.iter().any(|item| item.is_action(MenuAction::FixAll)));
        assert!(
            items
                .iter()
                .any(|item| matches!(item, DoctorMenuItem::Manual(_)))
        );
    }
}
