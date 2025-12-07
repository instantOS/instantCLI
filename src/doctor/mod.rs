use anyhow::Result;
use clap::Subcommand;
use colored::*;
use comfy_table::{Attribute, Cell, Color, ContentArrangement, Row, Table, presets::UTF8_FULL};
use indicatif::{ProgressBar, ProgressStyle};
use std::fmt::Display;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Subcommand, Debug, Clone)]
pub enum DoctorCommands {
    /// List all available health checks
    List,
    /// Run a specific health check
    Run {
        /// Name of the check to run
        name: String,
    },
    /// Apply fix for a specific health check
    Fix {
        /// Name of the check to fix
        name: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PrivilegeLevel {
    User, // Must run as normal user
    Root, // Must run as root
    Any,  // Can run as either
}

#[derive(Debug, Clone)]
pub enum CheckStatus {
    Pass(String),
    Warning { message: String, fixable: bool },
    Fail { message: String, fixable: bool },
    Skipped(String),
}

impl CheckStatus {
    pub fn message(&self) -> &String {
        match self {
            CheckStatus::Pass(msg) => msg,
            CheckStatus::Warning { message, .. } => message,
            CheckStatus::Fail { message, .. } => message,
            CheckStatus::Skipped(msg) => msg,
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(self, CheckStatus::Pass(_))
    }

    pub fn is_warning(&self) -> bool {
        matches!(self, CheckStatus::Warning { .. })
    }

    pub fn is_skipped(&self) -> bool {
        matches!(self, CheckStatus::Skipped(_))
    }

    pub fn is_fixable(&self) -> bool {
        match self {
            CheckStatus::Pass(_) => false,
            CheckStatus::Warning { fixable, .. } => *fixable,
            CheckStatus::Fail { fixable, .. } => *fixable,
            CheckStatus::Skipped(_) => false,
        }
    }

    pub fn needs_fix(&self) -> bool {
        matches!(self, CheckStatus::Fail { .. })
    }

    pub fn status_text(&self) -> &'static str {
        match self {
            CheckStatus::Pass(_) => "PASS",
            CheckStatus::Warning { .. } => "WARN",
            CheckStatus::Fail { .. } => "FAIL",
            CheckStatus::Skipped(_) => "SKIP",
        }
    }

    pub fn status_color(&self) -> Color {
        match self {
            CheckStatus::Pass(_) => Color::Green,
            CheckStatus::Warning { .. } => Color::Yellow,
            CheckStatus::Fail { .. } => Color::Red,
            CheckStatus::Skipped(_) => Color::DarkGrey,
        }
    }

    pub fn color_status(&self) -> impl std::fmt::Display {
        match self {
            CheckStatus::Pass(_) => "PASS".green(),
            CheckStatus::Warning { .. } => "WARN".yellow(),
            CheckStatus::Fail { .. } => "FAIL".red(),
            CheckStatus::Skipped(_) => "SKIP".dimmed(),
        }
    }

    pub fn fixable_indicator(&self) -> &'static str {
        match self {
            CheckStatus::Pass(_) => "",
            CheckStatus::Warning { fixable: true, .. } => " (fixable)",
            CheckStatus::Warning { fixable: false, .. } => "",
            CheckStatus::Fail { fixable: true, .. } => " (fixable)",
            CheckStatus::Fail { fixable: false, .. } => " (not fixable)",
            CheckStatus::Skipped(_) => "",
        }
    }

    /// Returns a priority value for sorting (lower = more important, shown first)
    /// Order: Fail (0) -> Warning (1) -> Pass (2) -> Skipped (3)
    pub fn sort_priority(&self) -> u8 {
        match self {
            CheckStatus::Fail { .. } => 0,
            CheckStatus::Warning { .. } => 1,
            CheckStatus::Pass(_) => 2,
            CheckStatus::Skipped(_) => 3,
        }
    }

    /// Get color for a status text string (for display purposes)
    pub fn color_for_status_text(status_text: &str) -> Color {
        match status_text {
            "PASS" => Color::Green,
            "WARN" => Color::Yellow,
            "FAIL" => Color::Red,
            "SKIP" => Color::DarkGrey,
            _ => Color::White,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CheckResult {
    pub name: String,
    pub check_id: String,
    pub status: CheckStatus,
    pub fix_message: Option<String>,
}

impl Display for CheckResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {} - {}",
            self.name.bold(),
            self.status.color_status(),
            self.status.message()
        )
    }
}

#[async_trait::async_trait]
pub trait DoctorCheck: Send + Sync {
    fn name(&self) -> &'static str;
    fn id(&self) -> &'static str; // Machine-readable identifier

    async fn execute(&self) -> CheckStatus;

    fn fix_message(&self) -> Option<String> {
        None
    }
    async fn fix(&self) -> Result<()> {
        Ok(())
    }

    // NEW: Privilege requirements
    fn check_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::Any
    }
    fn fix_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::Any
    }
}

pub mod checks;
pub mod command;
pub mod privileges;
pub mod registry;

pub async fn run_all_checks(checks: Vec<Box<dyn DoctorCheck + Send + Sync>>) -> Vec<CheckResult> {
    use privileges::skip_reason_for_privilege_level;
    use std::sync::Arc;
    use sudo::RunningAs;

    // Check privileges once before spawning any tasks
    let is_root = matches!(sudo::check(), RunningAs::Root);

    let total = checks.len();
    let completed = Arc::new(AtomicUsize::new(0));

    // Create progress bar
    let pb = ProgressBar::new(total as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} {msg} [{bar:30.cyan/blue}] {pos}/{len}")
            .unwrap()
            .progress_chars("█▓░"),
    );
    pb.set_message("Running health checks...");
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    let mut handles = vec![];
    for check in checks {
        let completed = Arc::clone(&completed);
        let pb = pb.clone();

        let handle = tokio::spawn(async move {
            let name = check.name().to_string();
            let check_id = check.id().to_string();
            let fix_message = check.fix_message();

            // Check privilege requirements before running
            let status = if let Some(skip_reason) =
                skip_reason_for_privilege_level(check.check_privilege_level(), is_root)
            {
                CheckStatus::Skipped(skip_reason.to_string())
            } else {
                check.execute().await
            };

            // Update progress
            let done = completed.fetch_add(1, Ordering::SeqCst) + 1;
            pb.set_position(done as u64);

            CheckResult {
                name,
                check_id,
                status,
                fix_message,
            }
        });
        handles.push(handle);
    }

    let mut results = vec![];
    for handle in handles {
        if let Ok(result) = handle.await {
            results.push(result);
        }
    }

    pb.finish_and_clear();
    results
}

// Unified table output functions
pub fn print_check_list_table(checks: &[Box<dyn DoctorCheck + Send + Sync>]) {
    use crate::ui::prelude::*;

    match get_output_format() {
        crate::ui::OutputFormat::Json => {
            let checks_data: Vec<_> = checks
                .iter()
                .map(|check| {
                    let fix_available = check.fix_message().is_some();
                    let privileges =
                        match (check.check_privilege_level(), check.fix_privilege_level()) {
                            (PrivilegeLevel::Any, PrivilegeLevel::Any) => "Any",
                            (PrivilegeLevel::Any, PrivilegeLevel::User) => "User (fix)",
                            (PrivilegeLevel::Any, PrivilegeLevel::Root) => "Root (fix)",
                            (PrivilegeLevel::User, PrivilegeLevel::User) => "User only",
                            (PrivilegeLevel::Root, _) => "Root required",
                            _ => "Mixed",
                        };

                    serde_json::json!({
                        "id": check.id(),
                        "name": check.name(),
                        "fix_available": fix_available,
                        "privileges": privileges,
                        "check_privilege": format!("{:?}", check.check_privilege_level()),
                        "fix_privilege": format!("{:?}", check.fix_privilege_level()),
                    })
                })
                .collect();

            emit(
                Level::Info,
                "doctor.check_list",
                "Doctor check list",
                Some(serde_json::json!({
                    "checks": checks_data,
                    "count": checks_data.len(),
                })),
            );
        }
        crate::ui::OutputFormat::Text => {
            let mut table = Table::new();
            table
                .load_preset(UTF8_FULL)
                .set_content_arrangement(ContentArrangement::Dynamic)
                .set_header(Row::from(vec![
                    Cell::new("ID")
                        .fg(Color::Blue)
                        .add_attribute(Attribute::Bold),
                    Cell::new("Name").add_attribute(Attribute::Bold),
                    Cell::new("Fix Available").add_attribute(Attribute::Bold),
                    Cell::new("Privileges").add_attribute(Attribute::Bold),
                ]));

            for check in checks {
                let fix = if check.fix_message().is_some() {
                    "✓"
                } else {
                    "✗"
                };
                let privileges = match (check.check_privilege_level(), check.fix_privilege_level())
                {
                    (PrivilegeLevel::Any, PrivilegeLevel::Any) => "Any",
                    (PrivilegeLevel::Any, PrivilegeLevel::User) => "User (fix)",
                    (PrivilegeLevel::Any, PrivilegeLevel::Root) => "Root (fix)",
                    (PrivilegeLevel::User, PrivilegeLevel::User) => "User only",
                    (PrivilegeLevel::Root, _) => "Root required",
                    _ => "Mixed",
                };

                table.add_row(Row::from(vec![
                    Cell::new(check.id()),
                    Cell::new(check.name()),
                    Cell::new(fix),
                    Cell::new(privileges),
                ]));
            }

            println!("{}", "Available Health Checks:".bold());
            println!("{table}");
            println!();
            let bin = env!("CARGO_BIN_NAME");
            println!("Usage:");
            println!("  {bin} doctor run <id>    Run a specific check");
            println!("  {bin} doctor fix <id>    Apply fix for a specific check");
            println!("  {bin} doctor             Run all checks");
        }
    }
}

pub fn print_results_table(results: &[CheckResult]) {
    use crate::ui::prelude::*;

    match get_output_format() {
        crate::ui::OutputFormat::Json => {
            let results_data: Vec<_> = results
                .iter()
                .map(|result| {
                    serde_json::json!({
                        "name": result.name,
                        "id": result.check_id,
                        "status": result.status.status_text(),
                        "success": result.status.is_success(),
                        "fixable": result.status.is_fixable(),
                        "message": result.status.message(),
                        "fixable_indicator": result.status.fixable_indicator(),
                        "fix_message": result.fix_message,
                    })
                })
                .collect();

            let success_count = results.iter().filter(|r| r.status.is_success()).count();
            let failure_count = results.iter().filter(|r| !r.status.is_success()).count();
            let fixable_count = results.iter().filter(|r| r.status.is_fixable()).count();

            emit(
                Level::Info,
                "doctor.results",
                "Doctor results",
                Some(serde_json::json!({
                    "results": results_data,
                    "summary": {
                        "total": results.len(),
                        "success": success_count,
                        "failures": failure_count,
                        "fixable": fixable_count,
                    }
                })),
            );
        }
        crate::ui::OutputFormat::Text => {
            // Sort results: Fail -> Warning -> Pass -> Skipped
            let mut sorted_results: Vec<_> = results.iter().collect();
            sorted_results.sort_by_key(|r| r.status.sort_priority());

            let mut table = Table::new();
            table
                .load_preset(UTF8_FULL)
                .set_content_arrangement(ContentArrangement::Dynamic)
                .set_header(Row::from(vec![
                    Cell::new("Check").add_attribute(Attribute::Bold),
                    Cell::new("Status").add_attribute(Attribute::Bold),
                    Cell::new("Message").add_attribute(Attribute::Bold),
                ]));

            for result in sorted_results {
                let status_cell =
                    Cell::new(result.status.status_text()).fg(result.status.status_color());
                let check_cell = Cell::new(&result.name).fg(result.status.status_color());

                let msg = format!(
                    "{}{}",
                    result.status.message(),
                    result.status.fixable_indicator()
                );

                table.add_row(Row::from(vec![check_cell, status_cell, Cell::new(msg)]));
            }

            println!("{}", "System Health Check Results:".bold());
            println!("{table}");

            // Show hint for skipped checks
            let skipped_results: Vec<_> =
                results.iter().filter(|r| r.status.is_skipped()).collect();
            if !skipped_results.is_empty() {
                let is_root = matches!(sudo::check(), sudo::RunningAs::Root);
                let bin = env!("CARGO_BIN_NAME");

                println!();
                if is_root {
                    // Running as root, some user-only checks were skipped
                    println!(
                        "{} {} checks were skipped because they cannot run as root.",
                        "Hint:".dimmed(),
                        skipped_results.len()
                    );
                    println!(
                        "      Run `{}` as a regular user to run those checks.",
                        bin.dimmed()
                    );
                } else {
                    // Running as user, some root-only checks were skipped
                    println!(
                        "{} {} checks were skipped because they require root privileges.",
                        "Hint:".dimmed(),
                        skipped_results.len()
                    );
                    println!(
                        "      Run `sudo {} doctor` to run those checks.",
                        bin.dimmed()
                    );
                }
            }
        }
    }
}

pub fn print_single_check_result_table(result: &CheckResult) {
    use crate::ui::prelude::*;

    match get_output_format() {
        crate::ui::OutputFormat::Json => {
            emit(
                Level::Info,
                "doctor.single_result",
                "Doctor single result",
                Some(serde_json::json!({
                    "name": result.name,
                    "id": result.check_id,
                    "status": result.status.status_text(),
                    "success": result.status.is_success(),
                    "fixable": result.status.is_fixable(),
                    "message": result.status.message(),
                    "fixable_indicator": result.status.fixable_indicator(),
                    "fix_message": result.fix_message,
                    "needs_fix": result.status.needs_fix(),
                })),
            );
        }
        crate::ui::OutputFormat::Text => {
            let mut table = Table::new();
            table
                .load_preset(UTF8_FULL)
                .set_content_arrangement(ContentArrangement::Dynamic)
                .set_header(Row::from(vec![
                    Cell::new("Check").add_attribute(Attribute::Bold),
                    Cell::new("Status").add_attribute(Attribute::Bold),
                    Cell::new("Message").add_attribute(Attribute::Bold),
                ]));

            let status_cell =
                Cell::new(result.status.status_text()).fg(result.status.status_color());
            let check_cell = Cell::new(&result.name).fg(result.status.status_color());

            let msg = format!(
                "{}{}",
                result.status.message(),
                result.status.fixable_indicator()
            );

            table.add_row(Row::from(vec![check_cell, status_cell, Cell::new(msg)]));

            println!("{}", "Health Check Result:".bold());
            println!("{table}");

            if result.status.needs_fix() {
                if result.status.is_fixable() {
                    if let Some(ref msg) = result.fix_message {
                        println!();
                        println!("  Fix available: {msg}");
                        println!(
                            "  Run: {} doctor fix {}",
                            env!("CARGO_BIN_NAME"),
                            result.check_id
                        );
                    }
                } else {
                    println!();
                    println!("  Manual intervention required.");
                }
            }
        }
    }
}

pub fn print_fix_summary_table(check_name: &str, before_status: &str, after_status: &str) {
    use crate::ui::prelude::*;

    match get_output_format() {
        crate::ui::OutputFormat::Json => {
            emit(
                Level::Info,
                "doctor.fix_summary",
                "Doctor fix summary",
                Some(serde_json::json!({
                    "check": check_name,
                    "before_status": before_status,
                    "after_status": after_status,
                    "success": after_status == "PASS",
                })),
            );
        }
        crate::ui::OutputFormat::Text => {
            let mut table = Table::new();
            table
                .load_preset(UTF8_FULL)
                .set_content_arrangement(ContentArrangement::Dynamic)
                .set_header(Row::from(
                    vec!["Check", "Before Status", "After Status"]
                        .into_iter()
                        .map(|s| Cell::new(s).add_attribute(Attribute::Bold))
                        .collect::<Vec<_>>(),
                ));

            table.add_row(Row::from(vec![
                Cell::new(check_name),
                Cell::new(before_status).fg(CheckStatus::color_for_status_text(before_status)),
                Cell::new(after_status).fg(CheckStatus::color_for_status_text(after_status)),
            ]));

            println!("{}", "Fix Summary:".bold());
            println!("{table}");
        }
    }
}
