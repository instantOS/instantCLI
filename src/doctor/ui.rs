use super::CheckResult;
use crate::menu_utils::{
    ConfirmResult, FzfPreview, FzfResult, FzfSelectable, FzfWrapper, MenuCursor,
};
use crate::ui::catppuccin::{colors, format_icon_colored, fzf_mocha_args};
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

/// Wrapper enum for menu items that can be either an action or an issue
#[derive(Clone)]
pub enum DoctorMenuItem {
    Action(MenuAction),
    Issue(FixableIssue),
    #[allow(dead_code)]
    Check(ViewableCheck),
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

    pub fn is_action(&self, action: MenuAction) -> bool {
        match self {
            DoctorMenuItem::Action(a) => *a == action,
            DoctorMenuItem::Issue(issue) => issue.action == Some(action),
            DoctorMenuItem::Check(_) => false,
        }
    }

    #[allow(dead_code)]
    pub fn check_id(&self) -> Option<&str> {
        match self {
            DoctorMenuItem::Issue(issue) => issue.check_id.as_deref(),
            DoctorMenuItem::Check(check) => Some(&check.check_id),
            DoctorMenuItem::Action(_) => None,
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
                    format!(
                        "{} Close",
                        format_icon_colored(NerdFont::Cross, colors::OVERLAY1)
                    )
                }
            },
            DoctorMenuItem::Issue(issue) => issue.fzf_display_text(),
            DoctorMenuItem::Check(check) => check.fzf_display_text(),
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
                        .header(NerdFont::Cross, "Close")
                        .text("Exit the diagnostics menu")
                        .build(),
                }
            }
            DoctorMenuItem::Issue(issue) => issue.fzf_preview(),
            DoctorMenuItem::Check(check) => check.fzf_preview(),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            DoctorMenuItem::Action(action) => action.to_string(),
            DoctorMenuItem::Issue(issue) => issue.fzf_key(),
            DoctorMenuItem::Check(check) => check.fzf_key(),
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
                    format!(
                        "{} Close",
                        format_icon_colored(NerdFont::Cross, colors::OVERLAY1)
                    )
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
        use crate::ui::preview::PreviewBuilder;

        let icon = match self.status.as_str() {
            "FAIL" => NerdFont::CrossCircle,
            "WARN" => NerdFont::Warning,
            "ALL" => NerdFont::CheckCircle,
            "INFO" => NerdFont::Info,
            _ => NerdFont::Info,
        };

        let mut builder =
            PreviewBuilder::new().header(icon, &format!("{} {}", self.status, self.name));

        if self.action == Some(MenuAction::ViewAll) {
            builder = builder.text(&self.message);
        } else {
            builder = builder.field("Current Status", &self.message);

            if let Some(fix_msg) = &self.fix_message {
                builder = builder
                    .blank()
                    .line(colors::MAUVE, None, "Available Fix:")
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
}

impl ViewableCheck {
    pub fn from_check_result(result: &CheckResult) -> Self {
        Self {
            name: result.name.clone(),
            check_id: result.check_id.clone(),
            status: result.status.status_text().to_string(),
            message: result.status.message().to_string(),
        }
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

        PreviewBuilder::new()
            .header(icon, &format!("{} {}", self.status, self.name))
            .field("Status", &self.message)
            .blank()
            .subtext(&format!("ID: {}", self.check_id))
            .build()
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

    FzfWrapper::builder()
        .prompt("View results:")
        .header("All Check Results - Use arrow keys to navigate, ESC to return")
        .args(fzf_mocha_args())
        .select(viewable)?;

    Ok(())
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

/// Build menu items for the interactive fix menu when there are fixable issues
pub fn build_fix_menu_items(fixable_issues: Vec<FixableIssue>) -> Vec<DoctorMenuItem> {
    let count = fixable_issues.len();
    let mut menu_items = vec![DoctorMenuItem::view_all(), DoctorMenuItem::fix_all(count)];
    menu_items.extend(fixable_issues.into_iter().map(DoctorMenuItem::Issue));
    menu_items
}

/// Build menu items for the interactive menu when all checks pass
pub fn build_success_menu_items() -> Vec<DoctorMenuItem> {
    vec![DoctorMenuItem::view_all(), DoctorMenuItem::close()]
}

/// Run the interactive menu for when all checks pass
pub async fn run_success_menu(results: &[CheckResult]) -> Result<()> {
    let success_count = results.iter().filter(|r| r.status.is_success()).count();
    let skipped_count = results.iter().filter(|r| r.status.is_skipped()).count();

    let menu_items = build_success_menu_items();
    let mut cursor = MenuCursor::new();

    loop {
        let mut builder = FzfWrapper::builder()
            .prompt("Select:")
            .header(format!(
                "{} All systems operational!\n\n✓ {} checks passed\n⊘ {} checks skipped\n\nSelect an option or press Esc to exit",
                char::from(NerdFont::Check),
                success_count,
                skipped_count
            ))
            .args(fzf_mocha_args());

        if let Some(index) = cursor.initial_index(&menu_items) {
            builder = builder.initial_index(index);
        }

        match builder.select(menu_items.clone())? {
            FzfResult::Selected(item) if item.is_action(MenuAction::ViewAll) => {
                cursor.update(&item, &menu_items);
                show_all_check_results(results)?;
                continue;
            }
            FzfResult::Selected(item) => {
                cursor.update(&item, &menu_items);
            }
            _ => {}
        }
        return Ok(());
    }
}
