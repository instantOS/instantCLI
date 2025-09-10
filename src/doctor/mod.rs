use anyhow::{Result, anyhow};
use colored::*;
use std::fmt::Display;
use tokio::join;

#[derive(Debug, Clone)]
pub enum CheckStatus {
    Pass(String),
    Fail(String),
    Warning(String),
}

impl CheckStatus {
    pub fn message(&self) -> &String {
        match self {
            CheckStatus::Pass(msg) => msg,
            CheckStatus::Fail(msg) => msg,
            CheckStatus::Warning(msg) => msg,
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(self, CheckStatus::Pass(_))
    }

    pub fn color_status(&self) -> impl std::fmt::Display {
        match self {
            CheckStatus::Pass(_) => "PASS".green(),
            CheckStatus::Fail(_) => "FAIL".red(),
            CheckStatus::Warning(_) => "WARN".yellow(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CheckResult {
    pub name: String,
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

    async fn execute(&self) -> CheckStatus;

    fn fix_message(&self) -> Option<String> {
        None
    }

    async fn fix(&self) -> Result<()> {
        Ok(())
    }
}

pub mod checks;
pub use checks::*;

pub async fn run_all_checks(checks: Vec<Box<dyn DoctorCheck + Send + Sync>>) -> Vec<CheckResult> {
    let mut handles = vec![];
    for check in checks {
        let check_clone = check; // since moved into spawn
        let handle = tokio::spawn(async move {
            let name = check_clone.name().to_string();
            let status = check_clone.execute().await;
            let fix_message = check_clone.fix_message();
            CheckResult {
                name,
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

pub fn print_results(results: &[CheckResult]) {
    let header = format!(
        "{: <30} [{}] {}",
        "Check".bold(),
        "Status".bold(),
        "Message".bold()
    );
    println!("{}", header);

    let mut failed_checks = vec![];
    for result in results {
        let status_str = result.status.color_status();
        let line = format!(
            "{: <30} [{}] {}",
            result.name,
            status_str,
            result.status.message()
        );
        println!("{}", line);
        if !result.status.is_success() && result.fix_message.is_some() {
            failed_checks.push(result.clone());
        }
    }

    if !failed_checks.is_empty() {
        let fixes_msg = "\nAvailable fixes:".bold().yellow();
        println!("{}", fixes_msg);
        for result in &failed_checks {
            if let Some(ref msg) = result.fix_message {
                println!("  - {}: {}", result.name, msg);
            }
        }
        // TODO: Prompt for fixes
    }
}
