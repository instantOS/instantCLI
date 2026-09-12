use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{self, Write};
use std::process::{Command, Stdio};

use crate::common::shell::resolve_current_binary;
use crate::common::systemd::ServiceScope;
use crate::menu_utils::{FzfPreview, StreamingCommand, StreamingMenuItem};
use crate::preview::{PreviewId, preview_command};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemdServiceSelectionPayload {
    pub name: String,
    pub description: String,
    pub active: String,
    pub enabled: String,
    pub scope: ServiceScope,
}

pub fn list_command(scope: ServiceScope) -> StreamingCommand {
    StreamingCommand::new(resolve_current_binary())
        .arg("settings")
        .arg("internal-generate-systemd-list")
        .arg("--scope")
        .arg(scope.as_str())
}

pub fn generate_and_print_list(scope: ServiceScope) -> Result<()> {
    stream_services(scope)
}

fn stream_services(scope: ServiceScope) -> Result<()> {
    let scope_args = scope.systemctl_args();

    let mut child = Command::new("systemctl")
        .args([
            "list-units",
            "--type=service",
            "--all",
            "--no-pager",
            "--plain",
            "--no-legend",
        ])
        .args(&scope_args)
        .stdout(Stdio::piped())
        .spawn()?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("Failed to capture systemctl stdout"))?;

    let reader = io::BufReader::new(stdout);

    let mut lines: Vec<(String, String, String)> = Vec::new();

    use std::io::BufRead;
    for line in reader.lines() {
        let line = line?;
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }

        let name = parts[0].replace(".service", "");
        let active = parts[2].to_string();
        let description = if parts.len() > 4 {
            parts[4..].join(" ")
        } else {
            String::new()
        };

        lines.push((name, active, description));
    }

    child.wait()?;
    let enabled_states = load_enabled_states(scope, lines.iter().map(|(name, _, _)| name));

    lines.sort_by(|a, b| a.0.cmp(&b.0));

    let scope_str = scope.as_str();

    let stdout = io::stdout();
    let mut handle = stdout.lock();

    for (name, active, description) in lines {
        let enabled = enabled_state_for(&name, enabled_states.as_ref());
        let display = format_display(&name, &active, &enabled, &description);
        let key = format!("{}:{}", name, scope_str);
        let row = StreamingMenuItem::new(
            "systemd-service",
            &key,
            display,
            SystemdServiceSelectionPayload {
                name,
                description,
                active,
                enabled,
                scope,
            },
        )
        .preview(FzfPreview::Command(preview_command(
            PreviewId::SystemdService,
        )))
        .encode()?;

        writeln!(handle, "{row}")?;
    }

    Ok(())
}

fn load_enabled_states<'a>(
    scope: ServiceScope,
    service_names: impl Iterator<Item = &'a String>,
) -> Option<HashMap<String, String>> {
    let scope_args = scope.systemctl_args();

    let mut command = Command::new("systemctl");
    command
        .arg("show")
        .arg("--property=Id,UnitFileState")
        .args(&scope_args)
        .args(service_names.map(|name| format!("{name}.service")));
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }

    Some(parse_enabled_states(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

fn parse_enabled_states(output: &str) -> HashMap<String, String> {
    output
        .split("\n\n")
        .filter_map(|block| {
            let mut name = None;
            let mut state = None;
            for line in block.lines() {
                if let Some(value) = line.strip_prefix("Id=") {
                    name = value.strip_suffix(".service");
                } else if let Some(value) = line.strip_prefix("UnitFileState=") {
                    state = Some(if value.is_empty() { "transient" } else { value });
                }
            }
            Some((name?.to_string(), state.unwrap_or("unknown").to_string()))
        })
        .collect()
}

fn enabled_state_for(name: &str, states: Option<&HashMap<String, String>>) -> String {
    states
        .map(|states| {
            states
                .get(name)
                .cloned()
                .unwrap_or_else(|| "transient".to_string())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn format_display(name: &str, active: &str, _enabled: &str, description: &str) -> String {
    use crate::ui::catppuccin::{colors, format_icon_colored};

    let (active_icon, active_color) = match active {
        "active" => (NerdFont::CheckCircle, colors::GREEN),
        "failed" => (NerdFont::CrossCircle, colors::RED),
        "inactive" => (NerdFont::Circle, colors::OVERLAY0),
        _ => (NerdFont::Question, colors::YELLOW),
    };

    use crate::ui::nerd_font::NerdFont;
    let icon_str = format_icon_colored(active_icon, active_color);
    let truncated = truncate(description, 40);

    format!("{}{} - {}", icon_str, name, truncated)
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}...", &s[..max_len - 3])
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_unit_file_states_in_one_snapshot() {
        let states = parse_enabled_states(
            "Id=alpha.service\nUnitFileState=enabled\n\nId=beta.service\nUnitFileState=static\n\nId=not-a-service.target\nUnitFileState=enabled\n",
        );
        assert_eq!(states.get("alpha").map(String::as_str), Some("enabled"));
        assert_eq!(states.get("beta").map(String::as_str), Some("static"));
        assert!(!states.contains_key("not-a-service"));
    }

    #[test]
    fn distinguishes_transient_units_from_failed_snapshot_queries() {
        let states = HashMap::new();
        assert_eq!(enabled_state_for("generated", Some(&states)), "transient");
        assert_eq!(enabled_state_for("generated", None), "unknown");
    }
}
