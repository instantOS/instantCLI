use anyhow::{Context, Result};
use std::process::Command;

use crate::menu_utils::FzfWrapper;

pub fn search_man_pages() -> Result<()> {
    let list_command = r#"
        man -w | tr ':' '\n' | while read -r path; do
            find "$path" -type f -name "man*" 2>/dev/null
        done | sed 's/.*\///; s/\.gz$//; s/\.[0-9].*//' | sort -u
    "#;

    let output = Command::new("bash")
        .arg("-c")
        .arg(list_command)
        .output()
        .context("Failed to generate man page list")?;

    if !output.status.success() {
        anyhow::bail!("Failed to list man pages");
    }

    let pages_str = String::from_utf8_lossy(&output.stdout);
    let pages: Vec<String> = pages_str.lines().map(String::from).collect();

    if pages.is_empty() {
        crate::assist::utils::show_notification("Man Pages", "No man pages found")?;
        return Ok(());
    }

    let selected = FzfWrapper::builder().prompt("Man Page").select(pages)?;

    if let crate::menu_utils::FzfResult::Selected(page) = selected {
        let command = format!(r#"man "{}""#, page);
        crate::assist::utils::run_command_in_terminal(&command, "Man Pages")?;
    }

    Ok(())
}
