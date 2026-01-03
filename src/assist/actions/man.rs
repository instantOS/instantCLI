use anyhow::{Context, Result};
use std::process::Command;

use crate::common::shell::shell_quote;
use crate::menu::client::MenuClient;
use crate::menu::protocol::{FzfPreview, SerializableMenuItem};

pub fn search_man_pages() -> Result<()> {
    let list_command = r#"
        man -w | tr ':' '\n' | while read -r path; do
            find "$path" -type f \( -name '*.[0-9]*' -o -name '*.[0-9]*.gz' \) 2>/dev/null
        done | sed 's/.*\///; s/\.[0-9].*//' | sort -u
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

    let client = MenuClient::new();
    let items: Vec<SerializableMenuItem> = pages
        .into_iter()
        .map(|page| SerializableMenuItem {
            display_text: page.clone(),
            preview: FzfPreview::Command(format!(
                "man -f {} 2>/dev/null || echo 'Manual page'",
                shell_quote(&page)
            )),
            metadata: None,
        })
        .collect();

    let selected = client.choice("Select a man page:".to_string(), items, false)?;

    // Handle empty selection (user cancelled)
    if selected.is_empty() {
        return Ok(());
    }

    let page = &selected[0].display_text;
    let command = format!(r#"man "{}""#, page);
    crate::assist::utils::run_command_in_terminal(&command, "Man Pages")?;

    Ok(())
}
