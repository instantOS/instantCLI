//! Integration between settings system and doctor CLI
//!
//! This module runs the doctor CLI, parses JSON output, and provides
//! an interactive menu for selecting fixes to apply.

use anyhow::{Context, Result};
use duct::cmd;
use serde_json::Value;
use crate::menu_utils::{FzfWrapper, FzfResult, FzfSelectable};

/// Struct representing a fixable issue for display in FZF menu
#[derive(Clone)]
pub struct FixableIssue {
    pub name: String,
    pub check_id: String,
    pub status: String,  // "FAIL" or "WARN"
    pub message: String,
    pub fix_message: Option<String>,
}

impl FzfSelectable for FixableIssue {
    fn fzf_display_text(&self) -> String {
        format!("{} - {}", self.status, self.name)
    }

    fn fzf_key(&self) -> String {
        self.check_id.clone()
    }
}

/// Run doctor checks and parse JSON output
pub fn run_doctor_checks() -> Result<Vec<FixableIssue>> {
    // Run doctor with JSON output
    let output = cmd!(
        env!("CARGO_BIN_NAME"),
        "doctor",
        "--output-format",
        "json"
    )
    .read()
    .context("running doctor checks")?;

    // Parse JSON output
    let json: Value = serde_json::from_str(&output)
        .context("parsing doctor JSON output")?;

    // Extract results
    let results = json["results"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Missing results array"))?;

    // Filter for fixable issues (FAIL or WARN with fixable=true)
    let fixable_issues: Vec<FixableIssue> = results
        .iter()
        .filter_map(|result| {
            let fixable = result.get("fixable").and_then(|v| v.as_bool()).unwrap_or(false);
            let success = result.get("success").and_then(|v| v.as_bool()).unwrap_or(true);

            // Include if fixable and not successful (FAIL or WARN)
            if fixable && !success {
                Some(FixableIssue {
                    name: result.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    check_id: result.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    status: result.get("status").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    message: result.get("message").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    fix_message: result.get("fix_message").and_then(|v| v.as_str()).map(String::from),
                })
            } else {
                None
            }
        })
        .collect();

    Ok(fixable_issues)
}

/// Show interactive fix menu with "Fix All" and individual issues
pub fn show_fix_menu(fixable_issues: Vec<FixableIssue>) -> Result<Vec<FixableIssue>> {
    if fixable_issues.is_empty() {
        return Ok(Vec::new());
    }

    // Add "Fix All" option at the top
    let mut menu_items = vec![FixableIssue {
        name: format!("Fix All Issues ({})", fixable_issues.len()),
        check_id: "__ALL__".to_string(),
        status: "ALL".to_string(),
        message: "Apply all available fixes".to_string(),
        fix_message: None,
    }];
    menu_items.extend(fixable_issues);

    // Show FZF multi-select menu
    match FzfWrapper::builder()
        .multi_select(true)
        .prompt("Select issues to fix:")
        .header("System Diagnostics - Fixable Issues\n\nSelect issues to fix or press Esc to cancel")
        .select(menu_items)?
    {
        FzfResult::MultiSelected(selected) => Ok(selected),
        FzfResult::Cancelled => Ok(Vec::new()),
        _ => Ok(Vec::new()),
    }
}

/// Execute fixes for selected issues by calling CLI
pub fn execute_fixes(issues: Vec<FixableIssue>) -> Result<()> {
    if issues.is_empty() {
        return Ok(());
    }

    // Check if "Fix All" was selected
    let fix_all = issues.iter().any(|i| i.check_id == "__ALL__");

    if fix_all {
        // Call CLI to fix all issues (handles all privilege escalation logic)
        cmd!(env!("CARGO_BIN_NAME"), "doctor", "fix", "--all")
            .run()
            .context("executing fix all")?;
    } else {
        // Fix each selected issue individually
        for issue in issues {
            if issue.check_id != "__ALL__" {
                cmd!(env!("CARGO_BIN_NAME"), "doctor", "fix", &issue.check_id)
                    .run()
                    .context(format!("executing fix for {}", issue.name))?;
            }
        }
    }

    Ok(())
}
