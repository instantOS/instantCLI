use anyhow::Result;
use clap::Subcommand;
use colored::*;
use comfy_table::{Attribute, Cell, Color, ContentArrangement, Row, Table, presets::UTF8_FULL};
use std::fmt::Display;

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
    Fail { message: String, fixable: bool },
}

impl CheckStatus {
    pub fn message(&self) -> &String {
        match self {
            CheckStatus::Pass(msg) => msg,
            CheckStatus::Fail { message, .. } => message,
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(self, CheckStatus::Pass(_))
    }

    pub fn is_fixable(&self) -> bool {
        match self {
            CheckStatus::Pass(_) => false,
            CheckStatus::Fail { fixable, .. } => *fixable,
        }
    }

    pub fn needs_fix(&self) -> bool {
        !self.is_success()
    }

    pub fn status_text(&self) -> &'static str {
        match self {
            CheckStatus::Pass(_) => "PASS",
            CheckStatus::Fail { .. } => "FAIL",
        }
    }

    pub fn status_color(&self) -> Color {
        match self {
            CheckStatus::Pass(_) => Color::Green,
            CheckStatus::Fail { .. } => Color::Red,
        }
    }

    pub fn color_status(&self) -> impl std::fmt::Display {
        match self {
            CheckStatus::Pass(_) => "PASS".green(),
            CheckStatus::Fail { .. } => "FAIL".red(),
        }
    }

    pub fn fixable_indicator(&self) -> &'static str {
        match self {
            CheckStatus::Pass(_) => "",
            CheckStatus::Fail { fixable: true, .. } => " (fixable)",
            CheckStatus::Fail { fixable: false, .. } => " (not fixable)",
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
    let mut handles = vec![];
    for check in checks {
        let check_clone = check; // since moved into spawn
        let handle = tokio::spawn(async move {
            let name = check_clone.name().to_string();
            let check_id = check_clone.id().to_string();
            let status = check_clone.execute().await;
            let fix_message = check_clone.fix_message();
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
    results
}

// Unified table output functions
pub fn print_check_list_table(checks: &[Box<dyn DoctorCheck + Send + Sync>]) {
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
        let privileges = match (check.check_privilege_level(), check.fix_privilege_level()) {
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

pub fn print_results_table(results: &[CheckResult]) {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(Row::from(vec![
            Cell::new("Check").add_attribute(Attribute::Bold),
            Cell::new("Status").add_attribute(Attribute::Bold),
            Cell::new("Message").add_attribute(Attribute::Bold),
        ]));

    // The Dynamic content arrangement will handle wrapping automatically

    for result in results {
        let status_cell = Cell::new(result.status.status_text()).fg(result.status.status_color());

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
}

pub fn print_single_check_result_table(result: &CheckResult) {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(Row::from(vec![
            Cell::new("Check").add_attribute(Attribute::Bold),
            Cell::new("Status").add_attribute(Attribute::Bold),
            Cell::new("Message").add_attribute(Attribute::Bold),
        ]));

    let status_text = match &result.status {
        CheckStatus::Pass(_) => "PASS",
        CheckStatus::Fail { .. } => "FAIL",
    };
    let status_color = match &result.status {
        CheckStatus::Pass(_) => Color::Green,
        CheckStatus::Fail { .. } => Color::Red,
    };
    let status_cell = Cell::new(status_text).fg(status_color);

    let check_color = match &result.status {
        CheckStatus::Pass(_) => Color::Green,
        CheckStatus::Fail { .. } => Color::Red,
    };
    let check_cell = Cell::new(&result.name).fg(check_color);

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

pub fn print_fix_summary_table(check_name: &str, before_status: &str, after_status: &str) {
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
        Cell::new(before_status).fg(Color::Red),
        Cell::new(after_status).fg(Color::Green),
    ]));

    println!("{}", "Fix Summary:".bold());
    println!("{table}");
}
